# Automatic list updates

When Shiori recognizes what you are playing, it decides between three things: update your list by itself, ask you first, or do nothing.

## When nothing happens

Your list is left alone when:

- **You already watched it.** The episode number is at or below your progress, or the anime is Completed and you are not rewatching it.
- **The episode number makes no sense.** There is no number (and it is not a single-episode anime), or it is higher than the number of episodes the anime has.
- **The file is outside your library**, if you turned on "ignore files outside of library folders" and have at least one library folder.

"Now playing" tells you which of these applies.

## When you are asked

If the episode is more than one ahead of your progress, Shiori asks before updating. You watched episode 3 and now play episode 6: maybe you skipped two, maybe the file was recognized wrongly.

After the usual waiting time, a notification appears: the anime, the episode, and your current progress. You can confirm or decline in the app.

If you close the video before the waiting time is over, the question is dropped. If you close it afterwards, the question stays until you answer or play something else.

## When it updates by itself

In every other case, which is normally "you are playing the next episode", the update happens automatically, after a waiting time.

### The waiting time

Shiori waits before updating, so that opening a file for a moment does not count as watching it. The wait is two minutes by default and can be set anywhere from ten seconds to one hour.

While it waits you see the time remaining, and you can:

- **update now**, without waiting,
- **cancel**, so that this episode is not counted. You can still change your mind and update it while it keeps playing.

### Two ways to finish the wait

You choose one:

- **Update after the delay** (default). The list is updated as soon as the waiting time has passed.
- **Update when the player is closed.** After the waiting time has passed, Shiori waits until you close the video and updates then. Closing it before the waiting time is over does not count.

### Pausing the wait

With "pause while the player is not in focus" turned on, the waiting time only runs while the media player is the window you are working in. Switching to another program stops the clock; switching back continues it.

## What exactly is updated

The episode is marked as watched, with everything that follows from it (status, dates, rewatch count). See [Your anime list](anime-list.md).

If the file covers several episodes at once (episodes 1 to 3 in one file), your progress becomes the last of them.

After the update, "now playing" shows that it happened. Nothing more is done for this video, however long it keeps playing.

## One update per episode

Each episode is handled once. The next episode starts a new wait with a fresh clock. If the same video is still open and Shiori merely recognizes it late (for instance because you picked the anime by hand), the time already waited counts.

## Changing your mind afterwards

An automatic update is an ordinary edit. You can set the progress back by hand, and it appears in your history, where you can see what was changed and when.
