# Detecting what you watch

## What Shiori looks for

Every few seconds (three by default) Shiori checks whether a media player it knows is running and has a video open.

It comes with a list of known players: several dozen on Windows, including mpv, VLC and MPC-HC, and the common players on Linux. You can turn individual players off.

## Files

For a video file, Shiori reads the name and location of the file the player has open. It only needs the file name; it never reads the video itself.

A player often has other files open too, such as subtitles or audio tracks. Shiori takes the first one that is a video. If the player has files open but none of them is a video (you are listening to music, say), Shiori detects nothing.

Some players do not reveal their open files. For those, Shiori reads the title of the player's window instead.

## Streaming in a web browser

Detecting video played on a website is off by default. When you turn it on, Shiori reads the title of the page in your browser and treats it like a file name.

Shiori never sees what you watch, your browsing history, or pages other than the one in the detected browser window.

## One thing at a time

If several players are running, Shiori follows one of them. It stays with the same player for as long as that player keeps playing, and does not jump between windows.

## Now playing

What Shiori currently detects is shown as "now playing", together with what will happen to your list. The tray icon shows a small mark: one color while the video is recognized as an anime, another while something is playing that Shiori could not recognize, and none when nothing is playing.

## When detection stops

When the player closes or stops having a video open, the next check notices and "now playing" clears.

## Turning detection off

Detection can be switched off entirely. Shiori then never looks at running programs, and your list only changes when you change it.

## When a player is not detected

- The player may not be on the list of known players.
- The player may be turned off in the settings.
- For browsers, streaming detection may be off.
- The player may be running with higher privileges than Shiori, which hides its open files.
