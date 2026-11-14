# Overview

## What Shiori is

Shiori is a desktop app for people who watch anime on their computer and keep a list of it: on AniList, MyAnimeList or Kitsu, or only on their computer.

It notices when you play an episode, works out which series and episode it is, and updates your list for you. You can also manage the list by hand, search for anime, and start the next episode of a series from inside the app.

It runs on Windows and Linux.

## How it works, in one paragraph

Every few seconds Shiori looks for a media player it knows. If one is playing a video, Shiori reads the file name, matches it to an anime, and checks whether this is the episode you should be watching next. If so, it waits a couple of minutes to be sure you are really watching, then marks the episode as watched and sends the change to your list service.

## Words used in this handbook

| Word | Meaning |
| --- | --- |
| List service | AniList, MyAnimeList or Kitsu: the website where your list lives |
| Anime list | Your list on the service: which anime you watch, how far you are, your scores |
| List entry | One anime in your list, with your progress, score, status, dates and notes |
| Progress | The number of episodes you have watched of an anime |
| Media player | A program that plays video, such as mpv, VLC or MPC-HC |
| Detection | Shiori noticing that a media player is playing something |
| Recognition | Working out which anime and episode a file is |
| Library folder | A folder on your computer where you keep anime files |
| Sync queue | Changes you made that have not reached the list service yet |
| Now playing | The video Shiori currently detects |

## What the first release includes

- Managing your anime list: add, edit, remove.
- Searching for anime and browsing a season.
- Details for each anime: titles, episodes, synopsis, genres, studios.
- Detecting media players and updating your list automatically.
- Detecting streaming in web browsers, if you turn it on.
- Library folders and playing episodes from them.
- A history of what you watched.
- One list service active at a time: AniList, MyAnimeList or Kitsu.
- Working offline: changes are kept and sent later.
- Keeping your list on this computer only, without an account.
- Exporting your list as a text document or a MyAnimeList XML file, and importing one into a list kept on this computer.

## What it does not include yet

These exist in Taiga and may come to Shiori later, one at a time:

- Finding and downloading new episodes through torrent feeds.
- Showing what you watch in Discord.
- Statistics about your list.
- macOS.

## Relationship to Taiga

Shiori is a new program that follows the behavior of [Taiga](https://taiga.moe). It is not made by Taiga's author. Like Taiga, it is free software under the GNU General Public License version 3.

Shiori keeps its own data and does not read or change a Taiga installation. Your list itself lives on the list service, so it appears in Shiori the first time you synchronize.
