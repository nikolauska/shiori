# Settings

Every setting, what it does, and its starting value.

## Accounts

| Setting | Starts as | What it does |
| --- | --- | --- |
| Service | AniList | Which list service is active. Each service has its own separate list in Shiori. |
| Enable synchronization | On | Send your changes and fetch your list automatically. When off, this only happens when you choose "Synchronize". |
| Account details | Empty | Depends on the service. See [Synchronizing with your list service](sync.md). |

## Library

| Setting | Starts as | What it does |
| --- | --- | --- |
| Anime folders | None | Your library folders. See [Your library](library.md). |
| Title language | Japanese in Latin letters | Which title is shown: Japanese in Latin letters, English, or Japanese script. Falls back to Latin letters when an anime lacks the chosen one. |

## Application

| Setting | Starts as | What it does |
| --- | --- | --- |
| Color scheme | Follow the system | Light or dark appearance |

## Recognition

| Setting | Starts as | What it does |
| --- | --- | --- |
| Enable detection | On | Look for running media players. When off, your list only changes when you change it. |
| Detection interval | 3 seconds | How often Shiori looks |
| Media players | All on | Each known player can be turned off |
| Detect streaming media in web browsers | Off | Also recognize video played on websites |

## Anime list update

| Setting | Starts as | What it does |
| --- | --- | --- |
| When to update | After the delay | "After the delay": update as soon as the waiting time has passed. "When media is closed": update when you close the video, provided the waiting time has passed. |
| Delay | 120 seconds | The waiting time. Between 10 seconds and 1 hour. |
| Pause delay while player is not in focus | Off | The waiting time only runs while the media player is the window you are using |
| Ignore files outside of library folders | Off | Only files inside your library folders update your list |

These are explained in [Automatic list updates](list-updates.md).

Changes to these four settings apply from the next video. A video that is already being counted keeps the settings it started with.

## Advanced

| Setting | Starts as | What it does |
| --- | --- | --- |
| Proxy | None | Connect to the list service through a proxy: type (HTTP or SOCKS5), address, port, and optionally a username and password |

## Per-anime settings

Set from an anime's details window, not from the settings screen.

| Setting | What it does |
| --- | --- |
| Alternative titles | Extra titles for recognizing this anime's files. They take priority over all other titles. |
| Folder | Where this anime's files are |
| Display title | Your own name for this anime, shown everywhere |
