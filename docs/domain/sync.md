# Synchronizing with your list service

## List services

Shiori works with AniList and MyAnimeList; Kitsu is planned. One of them is active at a time. Each service has its own separate copy of your list inside Shiori, with its own library links, episode numbering and history; switching the active service switches to that copy and does not move anything between services. Switching back finds everything where you left it. You switch by picking the service's card under "Your list" in Settings.

MyAnimeList has no exact airing times, only a weekly time slot. With MyAnimeList as your service, Shiori asks AniList when the next episode of each airing anime in your list comes out, after every list fetch, so the Home schedule and the "new episodes" badges work the same as with AniList. Those requests carry MyAnimeList ids, nothing about you.

## Keeping your list on this computer

You don't need an account. On first start, without one, Shiori asks where your list should live: on this computer, in your AniList account, or in your MyAnimeList account. Choosing MyAnimeList opens the sign-in right away. You can change it later in Settings.

A list on this computer:

- Is saved as you change it, and that's all. Nothing is queued or sent, and Shiori never mentions synchronizing.
- Removes an anime at once when you remove it.
- Still gets anime information (titles, episodes, airing times, posters) from AniList, the same way searching does. Those requests carry anime ids or search words, nothing about you.
- Keeps that information fresh by itself: on start and every hour, Shiori asks AniList again about the anime you are watching or planning to watch and anything still airing, up to 50 at a time. An airing anime is asked about at most once an hour, a finished one at most once a week. "Refresh anime data" (where "Synchronize" is with an account) asks about all of them right away.

### Backups

Without an account, the list on your computer is the only copy, so Shiori backs it up once a week into the `backups` folder next to its database and keeps the last 4. The sidebar and Settings show when the last backup was made. In Settings you can also back up to a file of your choice at any time, turn the weekly backup off, or restore a backup.

Restoring replaces your whole list (entries, history, per-anime settings, title links and episode numbering) with the backup's. Shiori asks first, and saves the list as it was to `backups/before-restore.sqlite`, so restoring the wrong file can be undone. Files that are not Shiori backups, or that come from a newer Shiori, are refused and change nothing.

### Moving your list in and out

From Settings, a list on this computer can take in:

- A MyAnimeList XML file: what MyAnimeList, AniList and Taiga export, gzipped or not.
- A public AniList list, by its username. Nothing is signed in to and nothing is sent to that account.

Only anime that are not in your list yet are added; anything already there is left as it is, since it may have newer progress. Afterwards Shiori says how many were added, how many were already there, and which ones AniList does not have.

Any list can be exported as a MyAnimeList XML file, which MyAnimeList and AniList both import. Anime without a MyAnimeList id cannot be in it; Shiori names them after exporting.

It is kept apart from your account's list. Moving to this computer for the first time asks whether to start empty or with a copy of your current list; changes waiting to be sent are not copied. Moving back to your account opens that list as you left it, and the list on this computer stays, so you can switch back.

### Uploading to your account

When you move a list that has anime in it to your AniList or MyAnimeList account and are signed in, Shiori asks whether to upload it. If you are not signed in yet, sign in first; Settings then offers "Add it to your AniList list" (or MyAnimeList) for as long as the list on this computer exists.

Uploading first fetches your account's list, so it works with what is there now, then:

- Adds every anime the account does not have.
- For anime on both, moves progress and status forward only (Completed beats Watching; Watching, On hold and Dropped don't replace each other), and fills in score, notes and dates only where the account has none.
- Removes nothing from the account.

The list on this computer uses AniList's anime. Uploading to MyAnimeList matches each one by its MyAnimeList id; an anime MyAnimeList does not have stays only on this computer, and Shiori names it (or says how many there are) before and after uploading.

The changes are sent like any other edit. If the service cannot be reached, nothing is uploaded and Shiori says so.

## Signing in

| Service | How you sign in |
| --- | --- |
| AniList | You approve Shiori on the AniList website and give Shiori the access code AniList shows you |
| MyAnimeList | Shiori opens the MyAnimeList website in your browser; you approve Shiori there and the browser hands the sign-in back to Shiori by itself. If that fails (the browser says it can't connect), you copy the address from the browser's address bar into Shiori. |
| Kitsu | You enter your Kitsu email (or username) and password in Shiori |

Shiori keeps your sign-in details in your operating system's password store, not in a plain file.

Without signing in, Shiori can still show a public list if you enter the username, but it cannot change anything on the service.

AniList sign-ins eventually expire. Shiori then tells you to approve it again in the settings. MyAnimeList sign-ins renew themselves in the background; only if that fails does Shiori ask you to sign in again.

"Sign out" in Settings removes the stored sign-in. Your list and username stay, so a public list can still be shown.

## What synchronizing does

When Shiori starts, and whenever you choose "Synchronize":

- If you have changes waiting to be sent, they are sent.
- Otherwise your whole list is fetched from the service.

After a fetch, Shiori's copy matches the service: new entries appear, changed ones are updated, and entries that are no longer on the service are removed. The one exception is anything you changed in Shiori that has not been sent yet; that is kept.

## Your changes are saved first, sent second

Every change you make, and every automatic update, is stored on your computer immediately. Sending it to the list service happens in the background.

- Changes are sent about ten seconds after your last edit. Several edits to the same anime in a row go out as one.
- If sending fails (no connection, the service is down), the change stays waiting and Shiori tries again every five minutes.
- Waiting changes survive closing Shiori.
- Only what you actually changed is sent. Changing a score does not touch your progress on the service.

You can see the waiting changes, with the reason for the last failure, next to your history.

## Working offline

You can use Shiori without a connection. Your list is on your computer, edits are accepted, and automatic updates keep working. Everything is sent once the service can be reached.

If you are not signed in when a change is made, Shiori tells you the change is waiting.

## When both sides changed

- **You edit an anime while an earlier edit of the same anime is being sent.** Your newer edit wins and is sent next. The service's answer to the earlier edit does not overwrite it.
- **You remove an anime and add it back before the removal was sent.** It ends up in your list.
- **You change an entry on the website and in Shiori before synchronizing.** Shiori's waiting change is sent and wins for the fields you changed in Shiori. Fields you only changed on the website are picked up at the next fetch.
- **You delete an entry on the website.** It disappears from Shiori at the next fetch, unless you have a change for it waiting.

## Discarding waiting changes

You can clear the waiting changes. They are then never sent, and the next fetch replaces your local edits with what the service has. Shiori asks before doing this.

## Turning synchronization off

With automatic synchronization off, nothing is sent or fetched in the background. Changes still collect and are sent when you choose "Synchronize".

## Problems

| What you see | What it means |
| --- | --- |
| "Access token has expired" | Approve Shiori again in the account settings |
| "Cannot connect to server because of DDoS protection" | The service is blocking automated access for the moment; try later |
| "Anime list entry does not exist" | The entry was deleted on the service; synchronize to bring Shiori in line |
| "Anime removed from database" | The service deleted this anime; Shiori removed it too |
