# Install and update

You install one program, the **Symphony Launcher**. It downloads Symphony,
checks the download, installs it, keeps it up to date, and can go back to
the version you had before if an update breaks something.

## Download the launcher

Open [the Symphony site](https://tdemers218.github.io/symphony/) and
click **Download**. It picks the file for your computer. Every file is
also on the [releases page](https://github.com/tdemers218/symphony/releases):

| Computer | File |
| --- | --- |
| Any Mac (Apple silicon or Intel) | `symphony-launcher-macos.zip` |
| Windows | `symphony-launcher-x86_64-pc-windows-msvc.zip` |
| Linux | `symphony-launcher-x86_64-unknown-linux-gnu.tar.gz` |

## Open it the first time

Symphony isn't code-signed yet, so your system warns you the first time
you open the launcher. You only do this once.

**Mac**

1. Unzip the file and move **Symphony Launcher** to Applications.
2. Double-click it. When macOS says it can't verify the app, click
   **Done**.
3. Open **System Settings → Privacy & Security**, scroll down, and click
   **Open Anyway** next to Symphony Launcher. Confirm.

**Windows**

1. Unzip the file and run `symphony-launcher.exe`.
2. If SmartScreen appears, click **More info → Run anyway**.

**Linux**

Extract the archive and run `./symphony-launcher`.

## Install and launch

1. Click **Install Symphony**. The launcher downloads Symphony and
   verifies the download before installing it.
2. Click **Launch Symphony**.

From then on, open the launcher and click **Launch**.

## Update

When a new version is out, the launcher shows what changed and an
**Update** button.

You can also update without leaving Symphony: choose **Help → Update
Symphony…**. Symphony asks you to save unsaved edits, checks your release
channel, and installs the update. Click **Launch** when it's done.

The version you're running is shown at the bottom of the **Help** menu.

## If an update breaks something

The launcher keeps the version you had before. Pick it under **Installed
versions**, launch it, and
[tell us what went wrong](https://github.com/tdemers218/symphony/issues).

## Report a problem

- Bugs and ideas: [open an issue](https://github.com/tdemers218/symphony/issues).
  Say what you did, what you expected, and what happened. A screenshot or
  the project folder helps.
- Tablets: see [Pens and tablets](tablets.md#report-your-tablet).
