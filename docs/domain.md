# Shiori product handbook

What Shiori does, written for someone who uses it or has to explain it. No programming knowledge needed. How it is built is in the source code.

Shiori is not built yet. The handbook describes the agreed behavior of the first release, written as if it already works, so that it is ready when the app is. The behavior comes from Taiga, the app Shiori is a rewrite of.

## Topics

| Topic | Answers |
| --- | --- |
| [Overview](domain/overview.md) | What Shiori is, who it is for, the words it uses, what the first release includes |
| [Your anime list](domain/anime-list.md) | Statuses, progress, scores, dates, rewatching, removing |
| [Detecting what you watch](domain/detection.md) | Which players are noticed, files versus streaming, when nothing is detected |
| [Recognizing the anime](domain/recognition.md) | How a file name becomes a series and an episode, and what to do when it is wrong |
| [Automatic list updates](domain/list-updates.md) | When your list is updated by itself, when you are asked, when nothing happens |
| [Synchronizing with your list service](domain/sync.md) | AniList, MyAnimeList, Kitsu; working offline; what wins when both sides changed |
| [Your library](domain/library.md) | Library folders, playing the next episode |
| [Search and history](domain/search-and-history.md) | Finding anime, browsing a season, the watch history |
| [Settings](domain/settings.md) | Every setting and what it changes |

## Not decided yet

These are product questions without an answer. They are left out of the topics above until decided.

- What the Home page shows.
- What "Play random anime" picks from.
- Whether closing the window keeps Shiori running in the tray.
- On Linux desktops where Shiori cannot tell which window is active, what "pause while the player is not in focus" does.
- Whether the watch history and custom titles from an existing Taiga installation can be imported, and when.
