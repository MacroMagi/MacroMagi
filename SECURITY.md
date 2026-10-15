# Security

MacroMagi sends keyboard and mouse input, reads the screen, and can run Python code contained in a macro. Problems in those areas matter, and reports are welcome.

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue:

- Use **[Report a vulnerability](https://github.com/MacroMagi/MacroMagi/security/advisories/new)** on the repository's *Security* tab.

Include what you found, how to reproduce it, and which version or commit and operating system you used. You will get an acknowledgement, and a fix or an explanation before anything is made public.

## What counts

Reports about, for example:

- a `.magi` or `.mp` file that does something **merely by being opened** – running code, writing outside the data folder, crashing the app;
- MacroMagi writing anywhere outside its own data folder (other than a desktop shortcut you asked for);
- the Python engine being reachable by, or loading code from, anything other than MacroMagi itself.

## What is by design

- **Running a macro runs it.** A macro can click, type and, through a Python Script block, execute arbitrary Python with your permissions. Opening a macro is safe; pressing Run on one you have not read is your decision. Treat a `.magi` file from a stranger like a script from a stranger.
- **Recordings contain what was typed**, in readable form, inside the `.magi` file.
- **MacroMagi observes all keyboard and mouse input while it is open.** That is how global hotkeys and recording work. Input is only kept while you are recording, and nothing is ever sent over the network. The only network connection MacroMagi makes is downloading a background picture from an address you enter in *Settings → Appearance*.

## Supported versions

Until the first release, only the latest commit on `main` is supported.
