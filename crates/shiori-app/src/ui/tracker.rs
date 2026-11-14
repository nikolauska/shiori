//! Tracking in the UI: reads the detection thread's reports, keeps the
//! current media, feeds the update session and carries out what it asks.

use std::time::Duration;

use gpui::{Context, Task};
use jiff::Zoned;
use shiori_core::recognition::{MatchReason, Relations};
use shiori_core::{
    AnimeId, Episode, SessionInput, SessionOutput, UpdateSession, UpdateState,
    entry_with_episode_watched,
};
use shiori_detect::Player;

use shiori_app::detection::{self, Report};
use shiori_app::tracking::{
    Media, PlayerKey, choose_media, identify_media, is_outside_library, load_players,
    load_relations, update_settings,
};

use super::model::Model;

/// Whether the tray icon should show a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    None,
    Identified,
    Unidentified,
}

#[derive(Default)]
pub struct Tracker {
    session: UpdateSession,
    /// What is playing, identified or not.
    pub media: Option<Media>,
    /// The anime the user picked with "Set as now playing" for the media
    /// with these elements. Kept so that the next poll's identification
    /// does not undo the choice.
    manual: Option<(Vec<shiori_core::Element>, AnimeId)>,
    pub(super) relations: Relations,
    pub players: Vec<Player>,
    detection: Option<detection::Handle>,
    /// The last pass's results, to match focus reports to the media.
    keys: Vec<PlayerKey>,
    /// Whether the current media's window has the focus; `None` when the
    /// system cannot tell.
    focused: Option<bool>,
    /// The player's own playing or paused report, for when focus is
    /// unknown (decision I24).
    playing: Option<bool>,
    ticker: Option<Task<()>>,
    /// The badge last sent to the tray.
    pub badge: Option<Badge>,
}

impl std::fmt::Debug for Tracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tracker")
            .field("media", &self.media)
            .field("state", &self.session.state())
            .finish_non_exhaustive()
    }
}

impl Tracker {
    pub fn state(&self) -> UpdateState {
        self.session.state()
    }

    pub fn badge(&self) -> Badge {
        match &self.media {
            None => Badge::None,
            Some(media) if media.episode.anime_id.is_some() => Badge::Identified,
            Some(_) => Badge::Unidentified,
        }
    }

    /// Whether the countdown should run this second. Unknown focus counts
    /// as focused unless the player says it is paused.
    fn player_focused(&self) -> bool {
        self.focused.unwrap_or(self.playing != Some(false))
    }
}

/// What the model does when a confirmation is due; set by the UI so that
/// the model does not need to know about windows or the tray.
/// Gets the state and the anime's title.
pub type ConfirmHook = Box<dyn Fn(&UpdateState, &str, &mut Context<Model>)>;

impl Model {
    /// Loads the players and relations and starts the detection thread.
    pub fn start_tracking(&mut self, cx: &mut Context<Self>) {
        let (players, problem) = load_players(&self.startup.folders);
        if let Some(problem) = problem {
            self.set_error(format!("Players file: {problem}"), cx);
        }
        self.tracker.relations = load_relations(&self.startup.folders, &self.state().settings);
        self.tracker.players = players.clone();
        let settings = &self.state().settings;
        let interval = Duration::from_millis(settings.detection_interval_ms.max(1000).into());
        match detection::spawn(players, settings.detection_enabled, interval) {
            Ok((handle, mut reports)) => {
                self.tracker.detection = Some(handle);
                cx.spawn(async move |this, cx| {
                    while let Some(report) = reports.recv().await {
                        if this
                            .update(cx, |this, cx| this.on_detection(report, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => self.set_error(format!("Could not start detection: {error}"), cx),
        }
    }

    /// Passes changed detection settings to the thread. Players and
    /// streaming are applied on the next poll, which reads the settings.
    pub(super) fn detection_settings_changed(&mut self, cx: &mut Context<Self>) {
        let settings = &self.state().settings;
        let interval = Duration::from_millis(settings.detection_interval_ms.max(1000).into());
        if let Some(handle) = &self.tracker.detection {
            handle.configure(settings.detection_enabled, interval);
        }
        self.update_badge(cx);
    }

    fn on_detection(&mut self, report: Report, cx: &mut Context<Self>) {
        match report {
            Report::Found(results) => {
                let detected: Vec<_> = results.iter().map(|(d, _)| d.clone()).collect();
                let last = self.tracker.media.as_ref().map(|m| m.key);
                let media = choose_media(&detected, &self.state().settings, last);
                self.tracker.keys = detected.iter().map(PlayerKey::of).collect();
                if let Some(media) = &media
                    && let Some(i) = self.tracker.keys.iter().position(|k| *k == media.key)
                {
                    self.tracker.focused = results[i].1;
                    self.tracker.playing = detected[i].playing;
                }
                self.media_found(media, cx);
            }
            Report::Focus(focus) => {
                if let Some(media) = &self.tracker.media
                    && let Some(i) = self.tracker.keys.iter().position(|k| *k == media.key)
                    && let Some(&focused) = focus.get(i)
                {
                    self.tracker.focused = focused;
                }
            }
        }
    }

    fn media_found(&mut self, media: Option<Media>, cx: &mut Context<Self>) {
        let Some(mut media) = media else {
            if self.tracker.media.take().is_some() {
                self.tracker.manual = None;
                self.send_to_session(None, cx);
            }
            return;
        };
        identify_media(&mut media, self.state(), &self.tracker.relations);
        if let Some((elements, id)) = &self.tracker.manual {
            if *elements == media.episode.elements {
                media.episode.anime_id = Some(*id);
                media.matched = Some(MatchReason::Manual);
            } else {
                self.tracker.manual = None;
            }
        }
        // Only a different episode is news; the same file found again
        // every few seconds is not.
        let changed = self.tracker.media.as_ref().is_none_or(|old| {
            old.episode.anime_id != media.episode.anime_id
                || old.episode.elements != media.episode.elements
        });
        let episode = media.episode.clone();
        self.tracker.media = Some(media);
        if changed {
            self.send_to_session(Some(episode), cx);
        }
    }

    /// "Set as now playing": the playing media is this anime.
    pub fn set_now_playing(&mut self, id: AnimeId, cx: &mut Context<Self>) {
        let Some(media) = &mut self.tracker.media else {
            return;
        };
        media.episode.anime_id = Some(id);
        media.matched = Some(MatchReason::Manual);
        self.tracker.manual = Some((media.episode.elements.clone(), id));
        let episode = media.episode.clone();
        self.send_to_session(Some(episode), cx);
    }

    pub fn accept_update(&mut self, cx: &mut Context<Self>) {
        self.handle_session(SessionInput::Accept, cx);
    }

    pub fn cancel_update(&mut self, cx: &mut Context<Self>) {
        self.handle_session(SessionInput::Cancel, cx);
    }

    fn send_to_session(&mut self, episode: Option<Episode>, cx: &mut Context<Self>) {
        let settings = &self.state().settings;
        let outside_library = self
            .tracker
            .media
            .as_ref()
            .is_some_and(|m| is_outside_library(settings, &m.source));
        let input = SessionInput::EpisodeChanged {
            episode,
            settings: update_settings(settings),
            outside_library,
        };
        self.handle_session(input, cx);
    }

    fn handle_session(&mut self, input: SessionInput, cx: &mut Context<Self>) {
        let outputs = self.tracker.session.handle(input, &self.startup.state);
        for output in outputs {
            match output {
                SessionOutput::StateChanged(_) => {}
                SessionOutput::ConfirmationRequested(state) => {
                    let title = self
                        .tracker
                        .media
                        .as_ref()
                        .and_then(|m| m.episode.anime_id)
                        .map(|id| self.state().title(id))
                        .unwrap_or_default();
                    if let Some(hook) = self.confirm_hook.take() {
                        hook(&state, &title, cx);
                        self.confirm_hook = Some(hook);
                    }
                }
                SessionOutput::Commit { anime_id, number } => self.commit(anime_id, number, cx),
                SessionOutput::StartTicking => self.start_ticking(cx),
                SessionOutput::StopTicking => self.tracker.ticker = None,
            }
        }
        self.update_badge(cx);
        cx.notify();
    }

    fn start_ticking(&mut self, cx: &mut Context<Self>) {
        self.tracker.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let ticked = this.update(cx, |this, cx| {
                    let player_focused = this.tracker.player_focused();
                    this.handle_session(SessionInput::Tick { player_focused }, cx);
                });
                if ticked.is_err() {
                    break;
                }
            }
        }));
    }

    fn commit(&mut self, id: AnimeId, number: u32, cx: &mut Context<Self>) {
        let state = self.state();
        let Some(anime) = state.anime.get(&id) else {
            return;
        };
        let today = Zoned::now().date().into();
        let entry = entry_with_episode_watched(anime, state.entries.get(&id), number, today);
        let title = state.title(id);
        self.save_entry(entry, cx);
        self.set_status(format!("{title}: updated to episode {number}."), cx);
    }

    fn update_badge(&mut self, cx: &mut Context<Self>) {
        let badge = if self.state().settings.detection_enabled {
            self.tracker.badge()
        } else {
            Badge::None
        };
        if self.tracker.badge != Some(badge) {
            self.tracker.badge = Some(badge);
            cx.emit(TrackingEvent::Badge(badge));
        }
    }
}

/// Tracking news for the parts of the UI outside the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackingEvent {
    Badge(Badge),
}

impl gpui::EventEmitter<TrackingEvent> for Model {}
