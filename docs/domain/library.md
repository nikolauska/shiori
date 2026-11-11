# Your library

## Library folders

A library folder is a folder where you keep anime files. You can add as many as you like. Shiori looks inside them, including all folders within.

Library folders are used for three things:

- finding the file for an episode you want to play,
- showing your files with the anime and episode each one belongs to,
- optionally limiting automatic list updates to files inside them.

Shiori only reads file and folder names. It does not move, rename or change your files unless you ask it to.

## Playing an episode

From any anime you can play:

| Choice | Which episode |
| --- | --- |
| Next episode | The one after your progress. If you have watched them all, episode 1. |
| Last episode | The one you watched most recently |
| Random episode | A random one among those you have seen, plus the next one. For a completed anime, any episode. Unwatched episodes further ahead are never picked, to avoid spoilers. |
| A specific episode | You enter the number |

Shiori searches your library folders for a video file that it recognizes as that anime and that episode, and opens it with your default video player. A file containing several episodes counts for each of them.

If no file is found, Shiori says which episode of which anime it could not find.

Because the player then has the file open, Shiori detects it like any other playback and the usual update rules apply. See [Automatic list updates](list-updates.md).

## Opening the folder

"Open folder" opens the folder of an anime in your file manager. Shiori finds it by looking for a folder in your library whose name it recognizes as that anime.

## Browsing your files

The library view shows your library folders as a tree. Next to each file you see the anime and episode Shiori recognizes it as, the file type, size and date. This is the quickest way to spot files that are not recognized.

From there you can open a file, open the details of its anime, rename it, or delete it. Deleting asks first.

## Files outside the library

By default, playing any recognized file updates your list, wherever the file is.

With "ignore files outside of library folders" turned on, only files inside a library folder count. This is useful if you preview downloads in another folder and do not want that to mark episodes as watched. The option has no effect until you have added at least one library folder.
