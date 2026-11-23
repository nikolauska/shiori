# Settings

Every setting, what it does, and its starting value.

## Accounts

| Setting | Starts as | What it does |
| --- | --- | --- |
| Where your list lives | Asked on first start | "On this computer", "In my AniList account" or "In my MyAnimeList account". In Settings these are three cards under "Your list"; picking a service's card makes it the active service. See [Keeping your list on this computer](sync.md#keeping-your-list-on-this-computer). Without an account and before you answer, Shiori asks each time it starts. |
| Back up every week | On | For a list on this computer: back it up once a week, keeping the last 4. |
| Service | AniList | Which list service is active: the account card chosen under "Your list". Each service has its own separate list in Shiori. |
| Sync | On | Send your changes and fetch your list automatically. When off, this only happens when you choose "Synchronize". Hidden for a list on this computer. |
| Account details | Empty | Depends on the service. See [Synchronizing with your list service](sync.md). |

## Library

| Setting | Starts as | What it does |
| --- | --- | --- |
| Anime folders | None | Your library folders. See [Your library](library.md). |
| Titles in | Japanese in Latin letters (Romaji) | Which title is shown: Japanese in Latin letters, English, or Japanese script. Falls back to Latin letters when an anime lacks the chosen one. |

## Application

| Setting | Starts as | What it does |
| --- | --- | --- |
| Color scheme | Follow the system | Light or dark appearance |
| Tell me when a new version of Shiori is out | On | Shiori looks for a newer release when it starts and once a day while it runs, and shows a notice with a link to download it. It only tells you; it never downloads or installs anything by itself. Test versions (pre-releases) are not offered. |

## Media players

| Setting | Starts as | What it does |
| --- | --- | --- |
| Detect what I'm watching | On | Look for running media players. When off, your list only changes when you change it. |
| Every … s | 3 seconds | How often Shiori looks |
| Players | All on | Each known player can be turned off. A filter narrows the list. |
| Web browsers | Off | Also recognize video played on websites. The browsers are listed under it and can be turned off one by one. |

## Anime list update

| Setting | Starts as | What it does |
| --- | --- | --- |
| Update my list | After a delay | "After a delay": update as soon as the waiting time has passed. "When the player closes": update when you close the video, provided the waiting time has passed. |
| Wait … seconds first | 120 seconds | The waiting time. Between 10 seconds and 1 hour. |
| Pause the wait while the player is in the background | Off | The waiting time only runs while the media player is the window you are using |
| Only update for files in my library folders | Off | Only files inside your library folders update your list |

These are explained in [Automatic list updates](list-updates.md).

Changes to these four settings apply from the next video. A video that is already being counted keeps the settings it started with.

## Advanced

| Setting | Starts as | What it does |
| --- | --- | --- |
| Use a proxy | Off | Connect to the list service through a proxy: type (HTTP or SOCKS5), host, port, and optionally a username and password. Turning it off forgets the host. |

## Per-anime settings

Set from the Settings tab of an anime's inspector, not from the settings screen.

| Setting | What it does |
| --- | --- |
| Alternative titles | Extra titles for recognizing this anime's files. They take priority over all other titles. |
| Folder | Where this anime's files are |
| Display title | Your own name for this anime, shown everywhere |
