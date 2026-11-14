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

## When recognition is wrong or fails

**Nothing is recognized.** "Now playing" shows the title Shiori read but no anime. You can pick the right anime by hand; Shiori then treats what is playing as that anime.

**To fix it for good**, open the anime's settings and add the title from your file names as an alternative title. Your alternative titles always take priority.

**The wrong anime is recognized.** The anime's "Now playing" tab says which title Shiori read and what it matched. This usually happens when two anime share a title. Adding the title as an alternative title to the right one settles it.

## Finding files

The same recognition is used when Shiori searches your library folders for an episode to play. See [Your library](library.md).
