# Measuring many terminals

## What it does

`npm run measure` opens the built app with a throwaway config, drives it through fixed stages, and prints a markdown table of what it saw. The window is shown for the whole run, parked so only a thin strip of it sits at the right edge of the rightmost display.

1. Checks that the window is really drawing frames, and stops with an error if it is not.
2. Takes a memory reading with one terminal open.
3. Opens 24 terminals, the last two side by side, and takes a second memory reading.
4. Types into the newest terminal while nothing is printing anywhere.
5. Makes four terminals print as fast as they can (one beside the one being typed in, three hidden) and types into the same terminal again.
6. Fills eight terminals' scrollback completely and takes a third memory reading.
7. Switches through every terminal one at a time, forwards and then back.
8. Switches back and forth between two neighbouring terminals ten times.
9. Counts the terminals that lost their graphics context, and takes a last memory reading.

## Running it

```sh
npm run measure
npm run measure -- --terminals=6 --full=3 --flooding=2 --flood-seconds=2 --samples=10
```

The flags and their defaults are `--terminals=24`, `--full=8`, `--flooding=4`, `--flood-seconds=10` and `--samples=50`. `--samples` is how many keys are typed while nothing prints; while terminals print, keys are typed for the whole of `--flood-seconds`. Every value is a whole number of at least 1, `--flooding` is at most `--full`, and `--full` plus 2 is at most `--terminals`. The small run above takes under a minute and is for checking that the script still works; its numbers are not comparable with a default run.

Leave the window where it is while it runs. Hiding it, or switching to another desktop, stops it drawing; the run then fails and says so. The numbers also move with whatever else the machine is doing, so compare runs taken on a quiet machine.

The command builds the app first, so it always measures the code in the working tree. It writes the result as JSON to `dist.noindex/measure/<date>-<short commit>.json` and prints the table on stdout. It uses the same debug port as `npm run e2e`, so it refuses to start while anything listens on port 9422 and must never run at the same time as the e2e suite.

## Reading the numbers

Memory rows list each process as Electron reports it: its resident memory in megabytes. `Browser` is the main process, `Tab` is the window, `GPU` is the process that draws for it, and `Terminal runtime` is the process the shells run in. Memory shared between processes is counted in each of them, so the total is more than the machine really spends. The window's JavaScript heap is what the page itself holds, read after a garbage collection.

Typing delay runs from a key press to the first frame drawn after its echo arrived. The key goes in through Chromium's own input path, as a real one does, about ten times a second. A `cat` is running in the terminal being typed in, so the echo comes from the terminal driver and not from the shell.

Main process timer lag is how late a 5 ms timer in the main process fired. A busy main process makes it grow.

Frame gap is the time between two consecutive frames drawn by the window, and long tasks are stretches of more than 50 ms where the window's main thread did nothing else.

Output reaching the window is how many characters of terminal output arrived per second while the flooding terminals ran. Backlog after printing stopped is how long output kept arriving after the printing programs were interrupted.

A switch runs from the key press to the second frame drawn after the new entry is on screen. The first switch row goes through every entry in turn, so each terminal is shown for the second time with whatever graphics context it has left. The second switch row is two neighbouring entries, back and forth.

A lost graphics context means that terminal draws with xterm's slower renderer from then on, and Chromium allows a page about 16. The last two rows count terminals that lost theirs and terminals now on the slower renderer.

## Results

### Before the core rip out

Two runs on 2026-11-25, at commit `9d9f97f` plus this script. 24 terminals, 8 with a full scrollback, 4 printing for 10 s, 50 typing samples, scrollback 100000, window 1280x800. Machine: Apple M5 Max, 18 cores, 36 GB, macOS 26.7.3, on a display drawing about 240 frames a second.

What they show:

- **8 of the 24 terminals lost their graphics context**, in both runs, and stayed on the slower renderer. That is every terminal past Chromium's 16.
- **The window holds every scrollback.** It grows from 186 MB with 24 empty terminals to about 1,800 MB with 8 full ones, a little over 200 MB for each full terminal. The main process barely moves.
- **Printing does not hold up the main process.** Its timer lag stays under 4 ms while four terminals print flat out.
- **Printing does slow typing.** A key's echo takes 7 to 10 ms to draw when nothing prints, and about 105 to 120 ms at the 95th percentile while four terminals print.
- **Switching is quick**, about 6 ms, because every terminal is already built and only hidden.

The two runs differ most while printing: how fast `yes` gets its output through varied from 36 to 94 MB/s between them, and the typing delay and frame gap follow it. Read those rows with the MB/s row beside them.

| Measure | Run 1 | Run 2 |
| --- | --- | --- |
| Memory, one terminal: Browser | 160 | 160 |
| Memory, one terminal: GPU | 98 | 97 |
| Memory, one terminal: Network Service | 48 | 48 |
| Memory, one terminal: Tab | 140 | 140 |
| Memory, one terminal: total | 446 | 445 |
| Memory, one terminal: window's JavaScript heap | 11 | 11 |
| Memory, 24 terminals, each shown once: Browser | 163 | 163 |
| Memory, 24 terminals, each shown once: GPU | 115 | 114 |
| Memory, 24 terminals, each shown once: Network Service | 48 | 48 |
| Memory, 24 terminals, each shown once: Tab | 186 | 185 |
| Memory, 24 terminals, each shown once: total | 511 | 510 |
| Memory, 24 terminals, each shown once: window's JavaScript heap | 11 | 11 |
| Memory, 8 terminals with a full scrollback: Browser | 269 | 190 |
| Memory, 8 terminals with a full scrollback: GPU | 90 | 85 |
| Memory, 8 terminals with a full scrollback: Network Service | 42 | 41 |
| Memory, 8 terminals with a full scrollback: Tab | 1854 | 1805 |
| Memory, 8 terminals with a full scrollback: total | 2255 | 2121 |
| Memory, 8 terminals with a full scrollback: window's JavaScript heap | 11 | 11 |
| Memory, at the end: Browser | 175 | 161 |
| Memory, at the end: GPU | 95 | 98 |
| Memory, at the end: Network Service | 42 | 41 |
| Memory, at the end: Tab | 1736 | 1569 |
| Memory, at the end: total | 2047 | 1868 |
| Memory, at the end: window's JavaScript heap | 11 | 11 |
| Typing delay, nothing printing: median / 95th / worst (ms) | 7.0 / 12.5 / 14.3 | 10.1 / 10.8 / 10.9 |
| Typing delay, while printing: median / 95th / worst (ms) | 97.1 / 117.0 / 124.0 | 13.7 / 103.9 / 112.7 |
| Main process timer lag, nothing printing: median / 95th / worst (ms) | 0.7 / 0.8 / 8.7 | 0.7 / 0.8 / 2.1 |
| Main process timer lag, while printing: median / 95th / worst (ms) | 0.0 / 0.0 / 3.1 | 0.0 / 0.2 / 3.7 |
| Frame gap, nothing printing: median / 95th / worst (ms) | 4.2 / 4.5 / 24.1 | 4.2 / 4.3 / 4.7 |
| Frame gap, while printing: median / 95th / worst (ms) | 41.6 / 54.1 / 58.4 | 4.2 / 29.0 / 54.2 |
| Long tasks over 50 ms, nothing printing | 0 | 0 |
| Long tasks over 50 ms, while printing | 0 | 0 |
| Output reaching the window while printing (MB/s) | 93.5 | 36.3 |
| Backlog after printing stopped (ms) | 54.5 | 10.5 |
| Switch, every entry in turn: median / 95th / worst (ms) | 6.2 / 7.2 / 8.1 | 5.8 / 7.5 / 8.0 |
| Switch, between two entries: median / 95th / worst (ms) | 6.2 / 7.1 / 7.8 | 5.5 / 6.1 / 8.1 |
| Graphics contexts lost | 8 | 8 |
| Terminals on the slow renderer | 8 | 8 |

### After the runtime moved to its own process

Two runs on 2026-11-25, same machine and settings, with the shells in the app's terminal process and their output going straight from it to the window. Both runs printed at about 90 MB/s, so they compare with run 1 above.

What they show:

- **Typing, frames, switching and the main process's timer lag are unchanged.** A key's echo while four terminals print still takes about 100 ms at the median and 115 ms at the 95th percentile.
- **There is a new process, `Terminal runtime`**, at about 70 MB idle and up to 180 MB while printing.
- **The main process no longer carries terminal output.** While four terminals print it holds about 140 MB, where the build just before this one held 284 MB.
- **The window's memory peaks higher while terminals print flat out.** Right after the printing stage it is at about 2,400 MB, against 1,900 MB on the build just before this one and 1,850 MB in run 1 above. By the end of the run it is back to about 2,020 MB. The likely cause, not confirmed: output that the window has not got to yet used to wait in the main process, and now waits in the window's own process. Nothing slows a shell down for the window yet.
- The 8 lost graphics contexts are as before. That is for the step that stops the window holding every terminal.

| Measure | Run 1 | Run 2 |
| --- | --- | --- |
| Memory, one terminal: Browser | 160 | 160 |
| Memory, one terminal: GPU | 97 | 99 |
| Memory, one terminal: Network Service | 48 | 48 |
| Memory, one terminal: Terminal runtime | 67 | 67 |
| Memory, one terminal: Tab | 140 | 140 |
| Memory, one terminal: total | 512 | 514 |
| Memory, one terminal: window's JavaScript heap | 11 | 11 |
| Memory, 24 terminals, each shown once: Browser | 162 | 162 |
| Memory, 24 terminals, each shown once: GPU | 115 | 116 |
| Memory, 24 terminals, each shown once: Network Service | 48 | 48 |
| Memory, 24 terminals, each shown once: Terminal runtime | 71 | 71 |
| Memory, 24 terminals, each shown once: Tab | 190 | 189 |
| Memory, 24 terminals, each shown once: total | 585 | 587 |
| Memory, 24 terminals, each shown once: window's JavaScript heap | 11 | 11 |
| Memory, 8 terminals with a full scrollback: Browser | 138 | 149 |
| Memory, 8 terminals with a full scrollback: GPU | 87 | 101 |
| Memory, 8 terminals with a full scrollback: Network Service | 41 | 42 |
| Memory, 8 terminals with a full scrollback: Terminal runtime | 181 | 174 |
| Memory, 8 terminals with a full scrollback: Tab | 2443 | 2322 |
| Memory, 8 terminals with a full scrollback: total | 2890 | 2788 |
| Memory, 8 terminals with a full scrollback: window's JavaScript heap | 11 | 11 |
| Memory, at the end: Browser | 142 | 153 |
| Memory, at the end: GPU | 98 | 107 |
| Memory, at the end: Network Service | 41 | 42 |
| Memory, at the end: Terminal runtime | 136 | 174 |
| Memory, at the end: Tab | 2020 | 2023 |
| Memory, at the end: total | 2437 | 2499 |
| Memory, at the end: window's JavaScript heap | 11 | 11 |
| Typing delay, nothing printing: median / 95th / worst (ms) | 10.1 / 10.8 / 11.0 | 10.0 / 11.0 / 11.1 |
| Typing delay, while printing: median / 95th / worst (ms) | 100.8 / 115.4 / 141.2 | 101.5 / 115.6 / 117.2 |
| Main process timer lag, nothing printing: median / 95th / worst (ms) | 0.7 / 0.8 / 2.2 | 0.7 / 0.8 / 3.2 |
| Main process timer lag, while printing: median / 95th / worst (ms) | 0.7 / 0.8 / 4.8 | 0.7 / 0.8 / 3.1 |
| Frame gap, nothing printing: median / 95th / worst (ms) | 4.2 / 4.4 / 4.8 | 4.2 / 4.4 / 4.7 |
| Frame gap, while printing: median / 95th / worst (ms) | 41.7 / 54.2 / 66.7 | 41.7 / 54.2 / 62.5 |
| Long tasks over 50 ms, nothing printing | 0 | 0 |
| Long tasks over 50 ms, while printing | 0 | 0 |
| Output reaching the window while printing (MB/s) | 90.4 | 93.6 |
| Backlog after printing stopped (ms) | 58.8 | 112.3 |
| Switch, every entry in turn: median / 95th / worst (ms) | 6.3 / 7.3 / 7.6 | 6.3 / 7.0 / 7.2 |
| Switch, between two entries: median / 95th / worst (ms) | 6.3 / 6.8 / 7.4 | 6.3 / 7.0 / 14.3 |
| Graphics contexts lost | 8 | 8 |
| Terminals on the slow renderer | 8 | 8 |
