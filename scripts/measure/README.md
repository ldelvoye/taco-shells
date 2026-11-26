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

Output reaching the window is how many characters of terminal output arrived per second while the flooding terminals ran. A terminal the window has no view of sends it nothing, so since the window stopped keeping a view for every terminal this counts one of the four flooding terminals, the one on screen. Backlog after printing stopped is how long output kept arriving after the printing programs were interrupted.

A switch runs from the key press to the second frame drawn after the new entry is on screen. When the terminal switched to has no view, the clock runs on until its new view has taken in the runtime's snapshot. The first switch row goes through every entry in turn, so since the window stopped keeping a view for every terminal, almost every switch in it is to a terminal with no view, seven of them with a full scrollback. The second switch row is two neighbouring entries, back and forth, which both have views.

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

Two runs on 2026-11-25, same machine and settings, at the commit that added `src/runtime/main.ts`: the shells are in the app's terminal process and their output goes straight from it to the window. Both runs printed at about 90 MB/s, so of the two runs before the core rip out they compare with the first, which printed at 93.5 MB/s. One more run was made the same day on `main` at `dd677be`, the last commit with the shells in the main process; its numbers are in the small table below.

What they show:

- **Typing, frame gaps and switching are within a few milliseconds of before.** A key's echo while four terminals print takes about 100 ms at the median and 115 ms at the 95th percentile. The main process's timer lag stays under 5 ms.
- **There is a new process, `Terminal runtime`**, at about 70 MB with 24 idle terminals and about 180 MB after the scrollback fill.
- **The main process is smaller after the scrollback fill**: 138 and 149 MB, against 284 MB on `dd677be`. Terminal output no longer passes through it.
- **The window is larger after the scrollback fill, and all processes together are too.** The window is at 2,443 and 2,322 MB against 1,901 MB on `dd677be`, and the total is about 2,800 MB against 2,346 MB. At the end of the run the window is still about 120 MB above `dd677be`. Why is not established. One guess: output the window has not got to yet used to wait in the main process and now waits in the window's.
- **8 of the 24 terminals lose their graphics context**, as before.

| Measure, on `dd677be` | One run |
| --- | --- |
| Memory, 8 terminals with a full scrollback: Browser | 284 |
| Memory, 8 terminals with a full scrollback: Tab | 1901 |
| Memory, 8 terminals with a full scrollback: total | 2346 |
| Memory, at the end: Browser | 206 |
| Memory, at the end: Tab | 1901 |
| Memory, at the end: total | 2271 |
| Typing delay, nothing printing: median / 95th / worst (ms) | 7.8 / 10.8 / 10.9 |
| Typing delay, while printing: median / 95th / worst (ms) | 96.2 / 106.4 / 110.1 |
| Main process timer lag, while printing: median / 95th / worst (ms) | 0.0 / 0.0 / 3.6 |
| Output reaching the window while printing (MB/s) | 106.5 |
| Backlog after printing stopped (ms) | 69.4 |
| Switch, every entry in turn: median / 95th / worst (ms) | 6.3 / 7.1 / 7.7 |

The two runs at the new commit:

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

### After the runtime kept every screen

One run on 2026-11-25, same machine and settings, at the commit that added `src/runtime/screens.ts`. At that commit the runtime holds a screen for every terminal and the window holds a view for every terminal. The comparisons below are with the two runs in the section above.

What it shows:

- **The runtime holds about as much as the window.** After the scrollback fill the runtime is at 1,868 MB and the window at 1,779 MB, and all processes together at 3,918 MB, against about 2,800 MB.
- **Less output reaches the window**: 55 MB/s, against about 90 MB/s. The backlog after printing stopped is 7 ms, against 59 and 112 ms. This commit pauses a shell while more than a million characters of its output wait to be parsed, which is the likely cause; the run does not measure the pauses.
- **Typing while four terminals print is close to typing when nothing prints**: 11 ms at the median and 20 ms at the 95th percentile, against about 100 and 115 ms. The frame gap while printing is 4.2 ms at the median, against 41.7 ms. This is one run, and these rows move with the output rate, so read it as a direction and not as a size.
- Switching and the 8 lost graphics contexts are as before.

| Measure | One run |
| --- | --- |
| Memory, one terminal: Browser | 160 |
| Memory, one terminal: GPU | 99 |
| Memory, one terminal: Network Service | 48 |
| Memory, one terminal: Terminal runtime | 76 |
| Memory, one terminal: Tab | 141 |
| Memory, one terminal: total | 524 |
| Memory, one terminal: window's JavaScript heap | 11 |
| Memory, 24 terminals, each shown once: Browser | 162 |
| Memory, 24 terminals, each shown once: GPU | 116 |
| Memory, 24 terminals, each shown once: Network Service | 48 |
| Memory, 24 terminals, each shown once: Terminal runtime | 102 |
| Memory, 24 terminals, each shown once: Tab | 188 |
| Memory, 24 terminals, each shown once: total | 616 |
| Memory, 24 terminals, each shown once: window's JavaScript heap | 11 |
| Memory, 8 terminals with a full scrollback: Browser | 141 |
| Memory, 8 terminals with a full scrollback: GPU | 89 |
| Memory, 8 terminals with a full scrollback: Network Service | 40 |
| Memory, 8 terminals with a full scrollback: Terminal runtime | 1868 |
| Memory, 8 terminals with a full scrollback: Tab | 1779 |
| Memory, 8 terminals with a full scrollback: total | 3918 |
| Memory, 8 terminals with a full scrollback: window's JavaScript heap | 11 |
| Memory, at the end: Browser | 144 |
| Memory, at the end: GPU | 99 |
| Memory, at the end: Network Service | 40 |
| Memory, at the end: Terminal runtime | 1667 |
| Memory, at the end: Tab | 1702 |
| Memory, at the end: total | 3653 |
| Memory, at the end: window's JavaScript heap | 11 |
| Typing delay, nothing printing: median / 95th / worst (ms) | 10.1 / 10.6 / 11.1 |
| Typing delay, while printing: median / 95th / worst (ms) | 11.2 / 20.0 / 90.0 |
| Main process timer lag, nothing printing: median / 95th / worst (ms) | 0.7 / 0.8 / 2.3 |
| Main process timer lag, while printing: median / 95th / worst (ms) | 0.7 / 0.8 / 2.9 |
| Frame gap, nothing printing: median / 95th / worst (ms) | 4.2 / 4.5 / 4.8 |
| Frame gap, while printing: median / 95th / worst (ms) | 4.2 / 4.6 / 45.8 |
| Long tasks over 50 ms, nothing printing | 0 |
| Long tasks over 50 ms, while printing | 0 |
| Output reaching the window while printing (MB/s) | 55.4 |
| Backlog after printing stopped (ms) | 7.1 |
| Switch, every entry in turn: median / 95th / worst (ms) | 6.2 / 6.7 / 7.1 |
| Switch, between two entries: median / 95th / worst (ms) | 6.3 / 6.9 / 7.3 |
| Graphics contexts lost | 8 |
| Terminals on the slow renderer | 8 |

### After the window let go of hidden terminals

Two runs on 2026-11-25, same machine and settings, at the commit that added `src/window/terminal/shown.ts`. The window keeps a view for the terminals on screen and the most recently shown, eight in all, and the runtime keeps every terminal's screen. The comparisons below are with the two runs before the core rip out, at the top of these results.

What they show:

- **No terminal loses its graphics context.** It was 8 of 24 in every run before this one.
- **The window no longer holds every scrollback.** With eight full scrollbacks it is at about 355 MB, against about 1,830 MB. The runtime holds them: about 1,950 MB. All processes together are at about 2,600 MB.
- **The window's size at the end is not a saving.** After every terminal has been shown in turn the window is at 1,405 and 1,572 MB. It never has more than eight views, but seven of the terminals shown one after another have full scrollbacks, so for a while it holds seven of them, and its resident size stays near that mark afterwards. A separate run over twelve terminals of 60,000 lines, cycled forty times, found the window level at about 1,450 MB with eight views throughout; how much of a process's resident size is free inside it was not measured.
- **Typing while four terminals print is the same as typing when nothing prints**: 10 ms at the median and 12 ms at the 95th percentile, against about 100 and 115 ms. Three of the four printing terminals have no view, so their output never reaches the window, and the runtime pauses a shell it cannot keep up with.
- **Switching to a terminal that has a view takes about 6 ms, as before.**
- **Switching to a terminal that has none takes about 20 ms at the median, and over half a second when its scrollback is full**: 544 and 836 ms at the 95th percentile. That is the runtime writing out 100,000 lines and the new view reading them in.

| Measure | Run 1 | Run 2 |
| --- | --- | --- |
| Memory, one terminal: Browser | 160 | 161 |
| Memory, one terminal: GPU | 101 | 101 |
| Memory, one terminal: Network Service | 48 | 48 |
| Memory, one terminal: Terminal runtime | 76 | 76 |
| Memory, one terminal: Tab | 141 | 141 |
| Memory, one terminal: total | 526 | 526 |
| Memory, one terminal: window's JavaScript heap | 11 | 11 |
| Memory, 24 terminals, each shown once: Browser | 162 | 162 |
| Memory, 24 terminals, each shown once: GPU | 114 | 117 |
| Memory, 24 terminals, each shown once: Network Service | 48 | 48 |
| Memory, 24 terminals, each shown once: Terminal runtime | 103 | 103 |
| Memory, 24 terminals, each shown once: Tab | 177 | 178 |
| Memory, 24 terminals, each shown once: total | 603 | 607 |
| Memory, 24 terminals, each shown once: window's JavaScript heap | 11 | 11 |
| Memory, 8 terminals with a full scrollback: Browser | 145 | 165 |
| Memory, 8 terminals with a full scrollback: GPU | 92 | 115 |
| Memory, 8 terminals with a full scrollback: Network Service | 41 | 48 |
| Memory, 8 terminals with a full scrollback: Terminal runtime | 2028 | 1900 |
| Memory, 8 terminals with a full scrollback: Tab | 351 | 360 |
| Memory, 8 terminals with a full scrollback: total | 2657 | 2587 |
| Memory, 8 terminals with a full scrollback: window's JavaScript heap | 11 | 11 |
| Memory, at the end: Browser | 145 | 162 |
| Memory, at the end: GPU | 97 | 114 |
| Memory, at the end: Network Service | 41 | 48 |
| Memory, at the end: Terminal runtime | 2057 | 1651 |
| Memory, at the end: Tab | 1405 | 1572 |
| Memory, at the end: total | 3746 | 3547 |
| Memory, at the end: window's JavaScript heap | 11 | 11 |
| Typing delay, nothing printing: median / 95th / worst (ms) | 10.2 / 10.7 / 10.9 | 9.4 / 11.0 / 12.0 |
| Typing delay, while printing: median / 95th / worst (ms) | 9.8 / 12.1 / 13.7 | 9.5 / 11.1 / 14.0 |
| Main process timer lag, nothing printing: median / 95th / worst (ms) | 0.7 / 0.8 / 4.3 | 0.8 / 1.5 / 4.4 |
| Main process timer lag, while printing: median / 95th / worst (ms) | 0.7 / 0.8 / 6.0 | 0.8 / 1.6 / 5.4 |
| Frame gap, nothing printing: median / 95th / worst (ms) | 4.2 / 4.3 / 4.8 | 4.2 / 4.4 / 4.8 |
| Frame gap, while printing: median / 95th / worst (ms) | 4.2 / 4.5 / 4.8 | 4.2 / 4.5 / 4.8 |
| Long tasks over 50 ms, nothing printing | 0 | 0 |
| Long tasks over 50 ms, while printing | 0 | 0 |
| Output reaching the window while printing (MB/s) | 12.8 | 5.1 |
| Backlog after printing stopped (ms) | 4.3 | 3.9 |
| Switch, every entry in turn: median / 95th / worst (ms) | 19.3 / 543.7 / 573.0 | 23.5 / 836.4 / 861.5 |
| Switch, between two entries: median / 95th / worst (ms) | 6.2 / 6.8 / 6.8 | 5.9 / 8.3 / 9.2 |
| Graphics contexts lost | 0 | 0 |
| Terminals on the slow renderer | 0 | 0 |
