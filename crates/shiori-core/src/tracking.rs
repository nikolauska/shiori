//! Automatic list updates: whether watching an episode may change the
//! list, what the entry becomes, and the countdown before it happens. See
//! docs/tracking.md.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::anime::{Anime, AnimeId};
use crate::date::FuzzyDate;
use crate::entry::{ListEntry, ListStatus, is_in_list};
use crate::episode::Episode;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UpdateTrigger {
    /// Update once the delay has passed.
    #[default]
    AfterDelay,
    /// Wait for the media to close after the delay has passed.
    OnPlayerClose,
}

/// The episode number watching `episode` sets: the last of its range, or 1
/// for an anime with one episode, since a movie's file rarely has a number.
pub fn watched_episode_number(episode: &Episode, anime: &Anime) -> Option<u32> {
    match episode.episode_number_range() {
        Some((_, last)) if last > 0 => Some(last),
        _ if anime.episode_count == Some(1) => Some(1),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Confirm,
    Deny,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reason {
    #[default]
    None,
    OutsideLibrary,
    InvalidEpisode,
    AlreadyWatched,
    SkipsAhead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateDecision {
    pub action: Action,
    pub reason: Reason,
}

impl UpdateDecision {
    const fn new(action: Action, reason: Reason) -> Self {
        Self { action, reason }
    }
}

fn watched_count(entry: Option<&ListEntry>) -> u32 {
    entry
        .filter(|e| is_in_list(Some(e)))
        .map_or(0, |e| e.watched_episodes)
}

/// Whether watching `episode` may update the list. The first rule that
/// applies wins; the order is Taiga's.
pub fn decide_update(
    episode: &Episode,
    anime: &Anime,
    entry: Option<&ListEntry>,
    outside_library: bool,
) -> UpdateDecision {
    use Action::*;
    if outside_library {
        return UpdateDecision::new(Deny, Reason::OutsideLibrary);
    }
    let Some(number) = watched_episode_number(episode, anime)
        .filter(|&n| anime.episode_count.is_none_or(|count| n <= count))
    else {
        return UpdateDecision::new(Deny, Reason::InvalidEpisode);
    };
    let in_list = entry.filter(|e| is_in_list(Some(e)));
    if in_list.is_some_and(|e| e.status == ListStatus::Completed && !e.rewatching) {
        return UpdateDecision::new(Deny, Reason::AlreadyWatched);
    }
    let watched = watched_count(entry);
    // A one-episode anime at "0 of 1" with a file without a number gives
    // 1 here, which is above 0 and passes; at "1 of 1" rule 3 denies it.
    if number < watched || (number == watched && anime.episode_count != Some(1)) {
        return UpdateDecision::new(Deny, Reason::AlreadyWatched);
    }
    let lowest = episode
        .episode_number_range()
        .map_or(number, |(first, _)| first.max(1));
    if lowest > watched + 1 {
        return UpdateDecision::new(Confirm, Reason::SkipsAhead);
    }
    UpdateDecision::new(Allow, Reason::None)
}

/// The entry after watching episode `number`, with the dates, status and
/// rewatch count that go with it.
pub fn entry_with_episode_watched(
    anime: &Anime,
    entry: Option<&ListEntry>,
    number: u32,
    today: FuzzyDate,
) -> ListEntry {
    let mut entry = entry
        .filter(|e| is_in_list(Some(e)))
        .cloned()
        .unwrap_or_else(|| ListEntry::new(anime.id));
    let is_final = anime.episode_count == Some(number);
    entry.watched_episodes = number;
    if number == 1 && entry.date_started.is_empty() {
        entry.date_started = today;
    }
    if is_final && entry.date_completed.is_empty() {
        entry.date_completed = today;
    }
    if entry.rewatching && is_final {
        entry.rewatching = false;
        entry.rewatched_times += 1;
    }
    if is_final {
        entry.status = ListStatus::Completed;
    } else if !entry.rewatching {
        entry.status = ListStatus::Watching;
    }
    entry
}

/// The settings a session reads when new media starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateSettings {
    /// Seconds.
    pub delay: u32,
    pub trigger: UpdateTrigger,
    pub pause_when_unfocused: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            delay: 120,
            trigger: UpdateTrigger::AfterDelay,
            pause_when_unfocused: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    /// Nothing to update.
    #[default]
    Idle,
    /// Not allowed; `reason` says why.
    Denied,
    /// Updates automatically after `remaining`.
    Countdown,
    /// The delay has passed; updates when the media closes.
    WaitingForClose,
    /// Updates if the user agrees.
    Confirming,
    /// The list has been updated.
    Committed,
    /// The user declined.
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdateState {
    pub phase: Phase,
    pub reason: Reason,
    /// Only meaningful in Countdown.
    pub remaining: Duration,
    /// The number that would be set; 0 when there is none.
    pub episode: u32,
    /// The watched count before.
    pub previous_episode: u32,
    /// The countdown is not running because the player is not focused.
    pub paused: bool,
}

/// What the app tells the session.
#[derive(Debug, Clone)]
pub enum SessionInput {
    /// New media, a change to it, or `None` when it closed.
    EpisodeChanged {
        episode: Option<Episode>,
        settings: UpdateSettings,
        outside_library: bool,
    },
    /// Once per second while ticking.
    Tick {
        player_focused: bool,
    },
    /// The user agrees, or asks to update now.
    Accept,
    Cancel,
}

/// What the session tells the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOutput {
    StateChanged(UpdateState),
    /// The delay passed for an update that needs the user's say.
    ConfirmationRequested(UpdateState),
    /// Save `entry_with_episode_watched(.., number, ..)` for the anime.
    Commit {
        anime_id: AnimeId,
        number: u32,
    },
    StartTicking,
    StopTicking,
}

/// The session's view of the database: the anime and the list entry, as
/// they are now. The list can change while media plays (a sync, an edit),
/// so the session asks every time instead of keeping a copy.
pub trait Lookup {
    fn anime(&self, id: AnimeId) -> Option<&Anime>;
    fn entry(&self, id: AnimeId) -> Option<&ListEntry>;
}

/// The countdown for the current media. It has no timer: the app sends a
/// tick each second while asked to, so a second spent paused is simply
/// not counted.
#[derive(Debug, Default)]
pub struct UpdateSession {
    episode: Option<Episode>,
    /// Seconds counted so far.
    elapsed: u32,
    settings: UpdateSettings,
    outside_library: bool,
    paused: bool,
    dismissed: bool,
    committed: bool,
    notified: bool,
    ticking: bool,
    state: UpdateState,
}

impl UpdateSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> UpdateState {
        self.state
    }

    pub fn episode(&self) -> Option<&Episode> {
        self.episode.as_ref()
    }

    pub fn handle(&mut self, input: SessionInput, lookup: &dyn Lookup) -> Vec<SessionOutput> {
        let mut out = Vec::new();
        match input {
            SessionInput::EpisodeChanged {
                episode: Some(episode),
                settings,
                outside_library,
            } => self.media_changed(episode, settings, outside_library, lookup, &mut out),
            SessionInput::EpisodeChanged { episode: None, .. } => {
                self.media_closed(lookup, &mut out)
            }
            SessionInput::Tick { player_focused } => {
                self.paused = self.settings.pause_when_unfocused && !player_focused;
                if !self.paused {
                    self.elapsed += 1;
                }
                self.evaluate(lookup, &mut out);
            }
            SessionInput::Accept => {
                if self.committed {
                    return out;
                }
                let Some((episode, anime, entry)) = self.current(lookup) else {
                    return out;
                };
                let decision = decide_update(episode, anime, entry, self.outside_library);
                if decision.action != Action::Deny {
                    self.commit(lookup, &mut out);
                }
            }
            SessionInput::Cancel => {
                if self.episode.is_none() || self.committed {
                    return out;
                }
                self.dismissed = true;
                self.evaluate(lookup, &mut out);
            }
        }
        out
    }

    fn media_changed(
        &mut self,
        episode: Episode,
        settings: UpdateSettings,
        outside_library: bool,
        lookup: &dyn Lookup,
        out: &mut Vec<SessionOutput>,
    ) {
        // Becoming identified, or identified as something after being
        // unknown, is the same media: the time watched so far still counts.
        let same = self.episode.as_ref().is_some_and(|old| {
            old.elements == episode.elements
                && (old.anime_id.is_none() || old.anime_id == episode.anime_id)
        });
        if !same {
            self.elapsed = 0;
            self.settings = settings;
            self.paused = false;
            self.dismissed = false;
            self.committed = false;
            self.notified = false;
        }
        self.episode = Some(episode);
        self.outside_library = outside_library;
        if !self.committed && !self.ticking {
            self.ticking = true;
            out.push(SessionOutput::StartTicking);
        }
        self.evaluate(lookup, out);
    }

    fn media_closed(&mut self, lookup: &dyn Lookup, out: &mut Vec<SessionOutput>) {
        self.stop_ticking(out);
        self.evaluate(lookup, out);
        if self.committed {
            return;
        }
        match self.state.phase {
            Phase::WaitingForClose => {
                self.commit(lookup, out);
                return;
            }
            // The question stays until answered once the delay has passed;
            // closing before that means the episode was not watched.
            Phase::Confirming if self.elapsed >= self.settings.delay => return,
            _ => {}
        }
        self.episode = None;
        self.dismissed = false;
        self.committed = false;
        self.set_state(UpdateState::default(), out);
    }

    fn current<'a>(
        &'a self,
        lookup: &'a dyn Lookup,
    ) -> Option<(&'a Episode, &'a Anime, Option<&'a ListEntry>)> {
        let episode = self.episode.as_ref()?;
        let id = episode.anime_id?;
        let anime = lookup.anime(id)?;
        Some((episode, anime, lookup.entry(id)))
    }

    fn evaluate(&mut self, lookup: &dyn Lookup, out: &mut Vec<SessionOutput>) {
        if self.committed {
            return;
        }
        let Some((episode, anime, entry)) = self.current(lookup) else {
            self.set_state(UpdateState::default(), out);
            return;
        };
        let decision = decide_update(episode, anime, entry, self.outside_library);
        let mut state = UpdateState {
            reason: decision.reason,
            episode: watched_episode_number(episode, anime).unwrap_or(0),
            previous_episode: watched_count(entry),
            ..UpdateState::default()
        };
        let delay_passed = self.elapsed >= self.settings.delay;
        match decision.action {
            Action::Allow if self.dismissed => state.phase = Phase::Cancelled,
            Action::Allow if !delay_passed => {
                state.phase = Phase::Countdown;
                state.remaining =
                    Duration::from_secs(u64::from(self.settings.delay - self.elapsed));
                state.paused = self.paused;
            }
            Action::Allow if self.settings.trigger == UpdateTrigger::OnPlayerClose => {
                state.phase = Phase::WaitingForClose;
            }
            Action::Allow => {
                self.commit(lookup, out);
                return;
            }
            Action::Confirm if self.dismissed => state.phase = Phase::Cancelled,
            Action::Confirm => state.phase = Phase::Confirming,
            Action::Deny => state.phase = Phase::Denied,
        }
        self.set_state(state, out);
        if state.phase == Phase::Confirming && delay_passed && !self.notified {
            self.notified = true;
            out.push(SessionOutput::ConfirmationRequested(state));
        }
    }

    fn commit(&mut self, lookup: &dyn Lookup, out: &mut Vec<SessionOutput>) {
        let Some((episode, anime, entry)) = self.current(lookup) else {
            return;
        };
        let Some(number) = watched_episode_number(episode, anime) else {
            return;
        };
        let state = UpdateState {
            phase: Phase::Committed,
            episode: number,
            previous_episode: watched_count(entry),
            ..UpdateState::default()
        };
        out.push(SessionOutput::Commit {
            anime_id: anime.id,
            number,
        });
        self.committed = true;
        self.stop_ticking(out);
        self.set_state(state, out);
    }

    fn stop_ticking(&mut self, out: &mut Vec<SessionOutput>) {
        if self.ticking {
            self.ticking = false;
            out.push(SessionOutput::StopTicking);
        }
    }

    fn set_state(&mut self, state: UpdateState, out: &mut Vec<SessionOutput>) {
        if self.state != state {
            self.state = state;
            out.push(SessionOutput::StateChanged(state));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::episode::{Element, ElementKind};

    const ID: AnimeId = AnimeId(1);
    const TODAY: FuzzyDate = FuzzyDate::new(2026, 11, 12);

    fn anime(count: Option<u32>) -> Anime {
        let mut anime = Anime::new(ID);
        anime.episode_count = count;
        anime
    }

    fn entry(status: ListStatus, watched: u32) -> ListEntry {
        let mut entry = ListEntry::new(ID);
        entry.status = status;
        entry.watched_episodes = watched;
        entry
    }

    fn episode(numbers: &[&str]) -> Episode {
        let mut elements = vec![Element::new(ElementKind::Title, "Show")];
        elements.extend(
            numbers
                .iter()
                .map(|n| Element::new(ElementKind::Episode, *n)),
        );
        Episode {
            anime_id: Some(ID),
            elements,
        }
    }

    fn decide(numbers: &[&str], count: Option<u32>, entry: Option<&ListEntry>) -> UpdateDecision {
        decide_update(&episode(numbers), &anime(count), entry, false)
    }

    #[test]
    fn watched_number() {
        assert_eq!(
            watched_episode_number(&episode(&["3"]), &anime(Some(12))),
            Some(3)
        );
        assert_eq!(
            watched_episode_number(&episode(&["1", "3"]), &anime(None)),
            Some(3)
        );
        assert_eq!(
            watched_episode_number(&episode(&[]), &anime(Some(1))),
            Some(1)
        );
        assert_eq!(
            watched_episode_number(&episode(&["x"]), &anime(Some(1))),
            Some(1)
        );
        assert_eq!(
            watched_episode_number(&episode(&[]), &anime(Some(12))),
            None
        );
        assert_eq!(watched_episode_number(&episode(&[]), &anime(None)), None);
    }

    #[test]
    fn decision_table() {
        use Action::*;
        let watching = |n| entry(ListStatus::Watching, n);
        let allow = UpdateDecision::new(Allow, Reason::None);
        let deny = |reason| UpdateDecision::new(Deny, reason);
        // 1: outside the library.
        assert_eq!(
            decide_update(&episode(&["1"]), &anime(Some(12)), None, true),
            deny(Reason::OutsideLibrary)
        );
        // 2: no number, or above the episode count.
        assert_eq!(decide(&[], Some(12), None), deny(Reason::InvalidEpisode));
        assert_eq!(
            decide(&["13"], Some(12), None),
            deny(Reason::InvalidEpisode)
        );
        // With no known count any number is valid; 13 from 0 skips ahead.
        assert_eq!(
            decide(&["13"], None, None),
            UpdateDecision::new(Confirm, Reason::SkipsAhead)
        );
        // 3: completed and not rewatching.
        let completed = entry(ListStatus::Completed, 12);
        assert_eq!(
            decide(&["5"], Some(12), Some(&completed)),
            deny(Reason::AlreadyWatched)
        );
        let mut rewatching = entry(ListStatus::Completed, 4);
        rewatching.rewatching = true;
        assert_eq!(decide(&["5"], Some(12), Some(&rewatching)), allow);
        // 4: below the watched count.
        assert_eq!(
            decide(&["3"], Some(12), Some(&watching(4))),
            deny(Reason::AlreadyWatched)
        );
        // 5: equal to the watched count, unless the anime has one episode.
        assert_eq!(
            decide(&["4"], Some(12), Some(&watching(4))),
            deny(Reason::AlreadyWatched)
        );
        assert_eq!(decide(&[], Some(1), Some(&watching(0))), allow);
        assert_eq!(
            decide(&[], Some(1), Some(&entry(ListStatus::Completed, 1))),
            deny(Reason::AlreadyWatched)
        );
        // 6: skips ahead.
        assert_eq!(
            decide(&["6"], Some(12), Some(&watching(4))),
            UpdateDecision::new(Confirm, Reason::SkipsAhead)
        );
        // A range starting at the next episode does not skip.
        assert_eq!(decide(&["5", "7"], Some(12), Some(&watching(4))), allow);
        // 7: the next episode, also when not in the list.
        assert_eq!(decide(&["5"], Some(12), Some(&watching(4))), allow);
        assert_eq!(decide(&["1"], Some(12), None), allow);
        // An entry being removed counts as not in the list.
        let mut removed = watching(4);
        removed.pending_delete = true;
        assert_eq!(decide(&["1"], Some(12), Some(&removed)), allow);
    }

    #[test]
    fn entry_after_watching() {
        let first = entry_with_episode_watched(&anime(Some(12)), None, 1, TODAY);
        assert_eq!(first.watched_episodes, 1);
        assert_eq!(first.status, ListStatus::Watching);
        assert_eq!(first.date_started, TODAY);
        assert!(first.date_completed.is_empty());

        let mut planned = entry(ListStatus::PlanToWatch, 0);
        planned.date_started = FuzzyDate::new(2020, 1, 1);
        let next = entry_with_episode_watched(&anime(Some(12)), Some(&planned), 1, TODAY);
        assert_eq!(next.date_started, FuzzyDate::new(2020, 1, 1));
        assert_eq!(next.status, ListStatus::Watching);

        let last = entry_with_episode_watched(
            &anime(Some(12)),
            Some(&entry(ListStatus::Watching, 11)),
            12,
            TODAY,
        );
        assert_eq!(last.status, ListStatus::Completed);
        assert_eq!(last.date_completed, TODAY);

        let mut rewatch = entry(ListStatus::Completed, 11);
        rewatch.rewatching = true;
        rewatch.rewatched_times = 1;
        rewatch.date_completed = FuzzyDate::new(2020, 2, 2);
        let middle = entry_with_episode_watched(&anime(Some(12)), Some(&rewatch), 5, TODAY);
        assert_eq!(
            middle.status,
            ListStatus::Completed,
            "rewatching keeps the status"
        );
        assert!(middle.rewatching);
        let done = entry_with_episode_watched(&anime(Some(12)), Some(&rewatch), 12, TODAY);
        assert!(!done.rewatching);
        assert_eq!(done.rewatched_times, 2);
        assert_eq!(done.date_completed, FuzzyDate::new(2020, 2, 2));

        // Unknown episode count: never final.
        let open = entry_with_episode_watched(&anime(None), None, 30, TODAY);
        assert_eq!(open.status, ListStatus::Watching);
    }

    #[derive(Default)]
    struct Db {
        anime: HashMap<AnimeId, Anime>,
        entries: HashMap<AnimeId, ListEntry>,
    }

    impl Lookup for Db {
        fn anime(&self, id: AnimeId) -> Option<&Anime> {
            self.anime.get(&id)
        }
        fn entry(&self, id: AnimeId) -> Option<&ListEntry> {
            self.entries.get(&id)
        }
    }

    fn db(watched: u32) -> Db {
        let mut db = Db::default();
        db.anime.insert(ID, anime(Some(12)));
        db.entries.insert(ID, entry(ListStatus::Watching, watched));
        db
    }

    struct Run {
        session: UpdateSession,
        db: Db,
        out: Vec<SessionOutput>,
    }

    impl Run {
        fn new(watched: u32) -> Self {
            Self {
                session: UpdateSession::new(),
                db: db(watched),
                out: Vec::new(),
            }
        }

        fn send(&mut self, input: SessionInput) -> &mut Self {
            let out = self.session.handle(input, &self.db);
            self.out.extend(out);
            self
        }

        fn play(&mut self, episode: Episode, settings: UpdateSettings) -> &mut Self {
            self.send(SessionInput::EpisodeChanged {
                episode: Some(episode),
                settings,
                outside_library: false,
            })
        }

        fn close(&mut self) -> &mut Self {
            self.send(SessionInput::EpisodeChanged {
                episode: None,
                settings: UpdateSettings::default(),
                outside_library: false,
            })
        }

        fn ticks(&mut self, n: u32, focused: bool) -> &mut Self {
            for _ in 0..n {
                self.send(SessionInput::Tick {
                    player_focused: focused,
                });
            }
            self
        }

        fn phase(&self) -> Phase {
            self.session.state().phase
        }

        fn commits(&self) -> usize {
            self.out
                .iter()
                .filter(|o| matches!(o, SessionOutput::Commit { .. }))
                .count()
        }

        fn confirmations(&self) -> usize {
            self.out
                .iter()
                .filter(|o| matches!(o, SessionOutput::ConfirmationRequested(_)))
                .count()
        }
    }

    #[test]
    fn commits_after_the_delay_and_not_before() {
        let mut run = Run::new(4);
        run.play(episode(&["5"]), UpdateSettings::default());
        assert_eq!(run.out[0], SessionOutput::StartTicking);
        assert_eq!(run.phase(), Phase::Countdown);
        assert_eq!(run.session.state().remaining, Duration::from_secs(120));
        run.ticks(119, true);
        assert_eq!(run.phase(), Phase::Countdown);
        assert_eq!(run.session.state().remaining, Duration::from_secs(1));
        assert_eq!(run.commits(), 0);
        run.ticks(1, true);
        assert_eq!(run.phase(), Phase::Committed);
        assert!(run.out.contains(&SessionOutput::Commit {
            anime_id: ID,
            number: 5
        }));
        assert_eq!(
            run.out.last(),
            Some(&SessionOutput::StateChanged(run.session.state()))
        );
        assert!(run.out.contains(&SessionOutput::StopTicking));
        assert_eq!(run.session.state().previous_episode, 4);
    }

    #[test]
    fn unfocused_ticks_do_not_count_when_pausing() {
        let mut run = Run::new(4);
        let settings = UpdateSettings {
            pause_when_unfocused: true,
            ..UpdateSettings::default()
        };
        run.play(episode(&["5"]), settings)
            .ticks(10, true)
            .ticks(50, false);
        assert_eq!(run.session.state().remaining, Duration::from_secs(110));
        assert!(run.session.state().paused);
        run.ticks(1, true);
        assert_eq!(run.session.state().remaining, Duration::from_secs(109));
        assert!(!run.session.state().paused);

        // Without the setting focus does not matter.
        let mut run = Run::new(4);
        run.play(episode(&["5"]), UpdateSettings::default())
            .ticks(10, false);
        assert_eq!(run.session.state().remaining, Duration::from_secs(110));
    }

    #[test]
    fn on_player_close_commits_when_closed_after_the_delay() {
        let settings = UpdateSettings {
            trigger: UpdateTrigger::OnPlayerClose,
            ..UpdateSettings::default()
        };
        let mut run = Run::new(4);
        run.play(episode(&["5"]), settings).ticks(200, true);
        assert_eq!(run.phase(), Phase::WaitingForClose);
        assert_eq!(run.commits(), 0);
        run.close();
        assert_eq!(run.phase(), Phase::Committed);
        assert_eq!(run.commits(), 1);

        let mut run = Run::new(4);
        run.play(episode(&["5"]), settings).ticks(60, true).close();
        assert_eq!(run.phase(), Phase::Idle);
        assert_eq!(run.commits(), 0);
    }

    #[test]
    fn closing_during_the_countdown_resets() {
        let mut run = Run::new(4);
        run.play(episode(&["5"]), UpdateSettings::default())
            .ticks(60, true)
            .close();
        assert_eq!(run.phase(), Phase::Idle);
        assert!(run.session.episode().is_none());
        assert_eq!(run.commits(), 0);
        assert!(run.out.contains(&SessionOutput::StopTicking));
        // Starting again counts from zero.
        run.play(episode(&["5"]), UpdateSettings::default());
        assert_eq!(run.session.state().remaining, Duration::from_secs(120));
    }

    #[test]
    fn skipping_ahead_asks_once_after_the_delay() {
        let mut run = Run::new(4);
        run.play(episode(&["7"]), UpdateSettings::default());
        assert_eq!(run.phase(), Phase::Confirming);
        assert_eq!(run.session.state().reason, Reason::SkipsAhead);
        run.ticks(119, true);
        assert_eq!(run.confirmations(), 0);
        run.ticks(5, true);
        assert_eq!(run.confirmations(), 1);
        // Closed after the delay: the question stays.
        run.close();
        assert_eq!(run.phase(), Phase::Confirming);
        assert_eq!(run.confirmations(), 1);
        run.send(SessionInput::Accept);
        assert_eq!(run.phase(), Phase::Committed);
        assert!(run.out.contains(&SessionOutput::Commit {
            anime_id: ID,
            number: 7
        }));

        // Closed before the delay: dropped.
        let mut run = Run::new(4);
        run.play(episode(&["7"]), UpdateSettings::default())
            .ticks(30, true)
            .close();
        assert_eq!(run.phase(), Phase::Idle);
        assert_eq!(run.confirmations(), 0);
    }

    #[test]
    fn becoming_identified_keeps_the_time() {
        let mut run = Run::new(4);
        let mut unknown = episode(&["5"]);
        unknown.anime_id = None;
        run.play(unknown, UpdateSettings::default()).ticks(30, true);
        assert_eq!(run.phase(), Phase::Idle);
        run.play(episode(&["5"]), UpdateSettings::default());
        assert_eq!(run.phase(), Phase::Countdown);
        assert_eq!(run.session.state().remaining, Duration::from_secs(90));
    }

    #[test]
    fn a_new_episode_resets_time_and_dismissal() {
        let mut run = Run::new(4);
        run.play(episode(&["5"]), UpdateSettings::default())
            .ticks(30, true);
        run.send(SessionInput::Cancel);
        assert_eq!(run.phase(), Phase::Cancelled);
        run.ticks(200, true);
        assert_eq!(
            run.phase(),
            Phase::Cancelled,
            "cancelled for the rest of this media"
        );
        assert_eq!(run.commits(), 0);
        run.db.entries.insert(ID, entry(ListStatus::Watching, 5));
        run.play(episode(&["6"]), UpdateSettings::default());
        assert_eq!(run.phase(), Phase::Countdown);
        assert_eq!(run.session.state().remaining, Duration::from_secs(120));
    }

    #[test]
    fn cancel_then_accept_commits() {
        let mut run = Run::new(4);
        run.play(episode(&["5"]), UpdateSettings::default())
            .ticks(5, true);
        run.send(SessionInput::Cancel);
        assert_eq!(run.phase(), Phase::Cancelled);
        run.send(SessionInput::Accept);
        assert_eq!(run.phase(), Phase::Committed);
        assert_eq!(run.commits(), 1);
    }

    #[test]
    fn accept_does_nothing_when_denied() {
        let mut run = Run::new(4);
        run.play(episode(&["3"]), UpdateSettings::default());
        assert_eq!(run.phase(), Phase::Denied);
        run.send(SessionInput::Accept);
        assert_eq!(run.commits(), 0);
    }

    #[test]
    fn nothing_happens_after_commit_until_new_media() {
        let mut run = Run::new(4);
        run.play(episode(&["5"]), UpdateSettings::default())
            .send(SessionInput::Accept);
        assert_eq!(run.phase(), Phase::Committed);
        // The list now says 5, which would deny the same episode; the
        // committed state must stay anyway.
        run.db.entries.insert(ID, entry(ListStatus::Watching, 5));
        let before = run.out.len();
        run.ticks(10, true)
            .send(SessionInput::Accept)
            .send(SessionInput::Cancel)
            .play(episode(&["5"]), UpdateSettings::default())
            .close();
        assert_eq!(run.out.len(), before, "{:?}", &run.out[before..]);
        assert_eq!(run.phase(), Phase::Committed);
        assert_eq!(run.commits(), 1);
        // New media starts over.
        run.play(episode(&["6"]), UpdateSettings::default());
        assert_eq!(run.phase(), Phase::Countdown);
    }
}
