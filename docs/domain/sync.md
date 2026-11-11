# Synchronizing with your list service

## List services

Shiori works with AniList, MyAnimeList and Kitsu. One of them is active at a time. Each service has its own separate copy of your list inside Shiori; switching the active service switches to that copy and does not move anything between services.

## Signing in

| Service | How you sign in |
| --- | --- |
| AniList | You approve Shiori on the AniList website and give Shiori the access code AniList shows you |
| MyAnimeList | You approve Shiori on the MyAnimeList website |
| Kitsu | You enter your Kitsu email (or username) and password in Shiori |

Shiori keeps your sign-in details in your operating system's password store, not in a plain file.

Without signing in, Shiori can still show a public list if you enter the username, but it cannot change anything on the service.

AniList sign-ins eventually expire. Shiori then tells you to approve it again in the settings.

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
