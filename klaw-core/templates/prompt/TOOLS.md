# TOOLS.md - Local Notes

Skills define _how_ tools work. This file is for _your_ specifics — the stuff that's unique to your setup.

## What Goes Here

Things like:

- Camera names and locations
- SSH hosts and aliases
- Preferred voices for TTS
- Speaker/room names
- Device nicknames
- Anything environment-specific

## Examples

```markdown
### Cameras

- living-room → Main area, 180° wide angle
- front-door → Entrance, motion-triggered

### SSH

- home-server → 192.168.1.100, user: admin

### TTS

- Preferred voice: "Nova" (warm, slightly British)
- Default speaker: Kitchen HomePod
```

## Why Separate?

Skills are shared. Your setup is yours. Keeping them apart means you can update skills without losing your notes, and share skills without leaking your infrastructure.

Memory should not be stored here. Use the `memory` tool for durable memory, and keep this file only for local environment notes.

Assume this file may be inlined into prompts. Keep it concise, factual, and free of secrets.

Secrets should not be stored here. Put credentials in environment variables, secret managers, or local config files that are not inlined into prompts.

---

Add whatever helps you do your job. This is your cheat sheet.

If you rely on an external service, record the command shape, script path, env var names, or non-sensitive setup notes here. Do not store raw credentials.
