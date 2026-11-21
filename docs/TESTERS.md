# Testing Symphony

Thanks for helping test Symphony. You install one thing, the **Symphony
Launcher**. It installs Symphony, keeps it up to date, and can switch back
to the previous version if an update has a problem.

## 1. Download the launcher
Go to the [latest release](https://github.com/tdemers218/symphony/releases)
and download the launcher for your computer:

| Computer | File |
|---|---|
| Mac with Apple chip (M1 or newer) | `symphony-launcher-aarch64-apple-darwin.zip` |
| Mac with Intel chip | `symphony-launcher-x86_64-apple-darwin.zip` |
| Windows | `symphony-launcher-x86_64-pc-windows-msvc.zip` |
| Linux | `symphony-launcher-x86_64-unknown-linux-gnu.tar.gz` |

## 2. Open it for the first time
Symphony isn't code-signed yet, so your computer shows a warning the first
time. This happens once.

- **Mac**:
  1. Unzip the file and move **Symphony Launcher** to Applications.
  2. Double-click it. When macOS says it can't verify the app, click
     **Done**.
  3. Open **System Settings → Privacy & Security**. Scroll down and click
     **Open Anyway** next to Symphony Launcher. Then confirm.
- **Windows**:
  1. Unzip the file and run `symphony-launcher.exe`.
  2. If SmartScreen appears, click **More info → Run anyway**.
- **Linux**: extract the file and run `./symphony-launcher`.

## 3. Install and launch
1. Set **Updates** to **Preview (for testers)**.
2. Click **Install Symphony**. The launcher downloads it and checks the
   download before installing.
3. Click **Launch Symphony**.

If the launcher says "Symphony isn't installed yet" and "You have the
newest version" with no Install button, it's still on **Stable**, which
has no release yet. Switch **Updates** to **Preview (for testers)**.

Next time, open the launcher and click **Launch**. When an update is out,
the launcher shows what changed and an **Update** button.

## If an update breaks something
The launcher keeps the version you had before. Pick it under
**Installed versions**, launch it, and tell us what went wrong.

## Reporting
- Bugs and ideas: [open an issue](https://github.com/tdemers218/symphony/issues).
- Tablets: run the pen test and file a *Tablet report* (see
  [CONTRIBUTING.md](../CONTRIBUTING.md)).
