# Your anime list

## What an entry holds

Each anime in your list has:

| Field | Meaning |
| --- | --- |
| Status | Watching, Completed, On hold, Dropped, or Plan to watch |
| Progress | Episodes watched |
| Score | Your rating |
| Date started | When you began |
| Date completed | When you finished |
| Rewatching | Whether you are watching it again |
| Times rewatched | How many times you have watched it again |
| Notes | Free text |

Dates can be partly unknown. "2019" or "April 2019" are valid dates.

## Adding an anime

Find it through search, then add it with a status. It appears in your list at once and is sent to the list service in the background.

## Editing

You can change any field from the anime's menu or from its details window. Several anime can be selected and edited together.

For the two dates there are shortcuts:

- Date started: clear it, set it to the day the anime started airing, or set it to the day you last updated the entry.
- Date completed: clear it, set it to the day the anime finished airing, or set it to the day you last updated the entry.

Saving an entry without changing anything does nothing: nothing is sent and no history is written.

## Scores

Each list service has its own rating scale, and AniList and Kitsu let you choose between several. Shiori offers exactly the choices your service and your account settings allow, and shows scores the way the service does.

| Service | Scales |
| --- | --- |
| AniList | 100 points; 10 points with halves; 10 points; 5 stars; 3 smileys |
| MyAnimeList | 1 to 10, each with a name from "Appalling" to "Masterpiece" |
| Kitsu | Simple (Awful, Meh, Good, Great); stars with halves up to 5; 1 to 10 with halves |

A score of zero means "no score".

## What changes by itself when you watch an episode

When Shiori marks an episode as watched automatically:

- Progress becomes that episode's number.
- If it is episode 1 and you have no start date, the start date becomes today.
- If it is the last episode:
  - the status becomes Completed,
  - if you have no completion date, it becomes today,
  - if you were rewatching, rewatching ends and "times rewatched" goes up by one.
- If it is not the last episode and you are not rewatching, the status becomes Watching. This moves an anime out of Plan to watch, On hold or Dropped when you start watching it.

An anime that is not in your list is added to it when its first episode is marked as watched.

## Rewatching

"Start new rewatch" is offered for an anime that has finished airing and that you have watched completely. It turns rewatching on, sets progress back to zero, sets the status to Watching, and starts episode 1.

While rewatching, automatic updates work as for a first watch. Finishing the last episode ends the rewatch and counts it.

An anime marked Completed that you are not rewatching is never updated automatically, whatever you play.

## Removing an anime

Removing asks for confirmation. The anime disappears from your list at once. It is deleted from the list service in the background; until that has happened it is kept aside, so that the removal is not lost if you are offline.

If you add the anime again before the removal reached the service, it is simply added again.

## Titles

Anime have several titles: the Japanese title in Latin letters, an English title, the Japanese title in Japanese script, and often more. You choose which one Shiori shows. If the one you chose does not exist for an anime, the Latin-letter title is shown.

You can give any anime your own display title. It is then used everywhere in the app.

## Details kept up to date

Information about an anime (episode count, air dates, synopsis) is refreshed from the list service when you open its details and what Shiori has is old: more than an hour for anime that are still airing or have not started, more than a week for finished ones. You can refresh an anime by hand at any time.

If the list service has deleted an anime, it is removed from Shiori too and you are told.

## Exporting

You can save your list as a text document. It has one section per status and one line per anime with the title and your progress, sorted by title.

While MyAnimeList is your list service, you can also export in MyAnimeList's own import format.
