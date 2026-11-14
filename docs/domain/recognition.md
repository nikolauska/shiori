# Recognizing the anime

Once Shiori knows a file name or page title, it has to work out which anime and which episode it is.

## Reading the name

A typical file name:

```
[TaigaSubs]_Toradora!_(2008)_-_01v2_-_Tiger_and_Dragon_[1280x720_H.264_FLAC][1234ABCD].mkv
```

Shiori reads from it: the group that released it (TaigaSubs), the title (Toradora!), the year (2008), the episode (1), the episode's title, and technical details it does not need.

If the file name has no title in it, for example `01.mkv`, Shiori uses the name of the folder the file is in, and looks one more folder up if that folder is only called something like `Season 2`. Folders named `Anime`, `Downloads` or `Extras`, and your library folders themselves, are never taken as a title.

## Matching the title

Shiori compares the title with every anime it knows about: everything in your list, plus everything you have looked at through search.

Small differences in spelling do not matter. These are all treated as the same title:

- capital and small letters, accents, punctuation, spaces,
- `Season 2`, `2nd Season`, `S2` and plain `2`,
- Roman numerals and digits (`II` and `2`),
- `&` and `and`,
- the word `The`, and the words `The Animation`,
- `OVA`, `OAV` and `OAD`,
- the two common ways of writing certain Japanese sounds in Latin letters.

Each anime is known under several titles: its main titles in different languages, alternative titles from the list service, and the title with its year, such as `Toradora! (2008)`.

When more than one anime fits, your own alternative titles count most, then the main titles, then the service's alternative titles.

An anime that Shiori has never seen cannot be matched. If you start watching something completely new, look it up in search once and Shiori will know it from then on.

## Checking the episode

A match is only accepted if the episode makes sense for that anime:

- The episode number may not be higher than the number of episodes the anime has.
- Openings, endings and previews are not episodes and are ignored.
- A film or other single-episode anime needs no episode number.

## Continuous numbering

Some releases number a second season as a continuation of the first: episode 14 instead of season 2, episode 1. Shiori knows about more than a thousand such cases and translates the number, so episode 14 of the first season becomes episode 2 of the second.

The same list handles specials that are numbered as episode 0 or placed between regular episodes.

**When the list does not know the series**, Shiori looks at how AniList links the anime: its prequels and sequels. If the title matched an anime but the episode number is above its episode count, "now playing" asks, for example, "Is it Sasaki to Pii-chan Season 2, episode 4?". You can:

- **Use and remember**: Shiori saves this numbering and uses it for every later file of these seasons, without asking.
- **Use once**: only for what is playing now.
- **Review…**: open the numbering to check or change it first.
- **Dismiss**: leave the file unrecognized for now. Nothing is saved.

Shiori asks you to review instead of offering one answer when it cannot be sure: the seasons branch, an episode count is unknown, no season holds the number, or "Season 2 - 18" could count from the first season or from the second. Nothing in your list changes until you answer.

**Remembered numbering** lists the anime that share one numbering and the release numbers each covers, such as 1–12 and 13–24. Shiori uses it only for numbers above an anime's own episode count, so "Season 2 - 04" stays episode 4. Gaps mean no match. Movies, specials and side stories are left out unless you include them, for example a special numbered 0. You can make or edit one from the anime's settings ("Episode numbering..."), from the Library's menu on a file whose number is too high, and from Settings, Recognition, which also lists your title links. Forgetting a numbering asks first and never changes your list or history. Remembered numbering is kept on this computer only and is for AniList for now.

## When recognition is wrong or fails

**Nothing is recognized.** "Now playing" shows the title Shiori read, and offers the anime with the closest titles as one-click choices. "Link anime…" opens a picker: close matches, your list, and a search on AniList, each saying what would happen to your list if you picked it ("Next episode", "Already watched", "Adds to Watching"). While the picker is open, the countdown waits.

**Linking remembers by default.** The title from the file name, with its season when the file names one ("Kusuriya Season 3"), becomes an alternative title of the anime you picked, so later episodes are recognized on their own. A title links to one anime: linking it again moves it. Very short or vague titles ("S2", "OVA") are not remembered unless you tick the box, since they would match other anime's files too. Untick it to link only what is playing now. The anime's settings list which alternative titles came from linking; deleting one undoes it.

**The wrong anime is recognized.** The anime's "Now playing" tab says which title Shiori read and what it matched. This usually happens when two anime share a title, or a file leaves out the season. "Wrong anime?" (or "Change…") in "now playing", or "Set as now playing" on the right anime in your list, links it to the right one.

**Files in the library** that are not recognized can be linked the same way, from "Link to anime..." in the Library's menu.

## Finding files

The same recognition is used when Shiori searches your library folders for an episode to play. See [Your library](library.md).
