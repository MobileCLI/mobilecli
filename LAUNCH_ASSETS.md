# MobileCLI Launch Assets

Generated: March 14, 2026

---

## 1. SHOW HN POST

### Title Option A:
Show HN: MobileCLI – Stream AI coding agent terminals to your phone, approve tool calls from your couch

### Title Option B:
Show HN: MobileCLI – Self-hosted Rust daemon that puts Claude Code/Gemini CLI on your iPhone

### Body Text:

Hey HN, I built MobileCLI because I kept walking away from my desk while Claude Code was running and missing tool approval prompts. I'd come back 20 minutes later to find it had been sitting there waiting for me to type "y". So I built a thing.

MobileCLI is a small Rust daemon (~7k lines, async tokio) that watches your terminal sessions and streams them to an iOS app over a direct WebSocket connection. When an AI agent needs input — a tool call approval, a confirmation prompt, whatever — you get a push notification on your phone. You can read the context, approve or deny, and keep going. It works with Claude Code, Gemini CLI, OpenAI Codex, OpenCode, or really any interactive terminal program.

The important part: there's no cloud, no accounts, no relay servers. The daemon runs on your machine, and your phone connects directly over LAN or Tailscale. Your terminal data never leaves your network. The CLI daemon is MIT-licensed and open source (Rust). The iOS app is $20/yr or $30 lifetime — it includes a file browser/editor for quick fixes on the go. Install is just `curl -fsSL https://mobilecli.app/install.sh | bash`.

GitHub: https://github.com/MobileCLI/mobilecli
App Store: https://apps.apple.com/us/app/mobilecli/id6757689455
Site: https://mobilecli.app

Happy to answer questions about the architecture, the Rust side, or anything else.

---

## 2. PRODUCTHUNT LISTING

### Tagline (54 chars):
AI coding agents on your phone. No cloud required.

### Short Description (286 chars):
Stream Claude Code, Gemini CLI, and other AI agent terminal sessions to your iPhone. Get push notifications when agents need approval. Approve tool calls from anywhere. Self-hosted Rust daemon connects directly over LAN or Tailscale. No accounts, no cloud, no relay servers. Your data stays yours.

### Extended Description:

**The problem:** You kick off Claude Code or Gemini CLI on a big task, walk away to grab coffee, and come back to find your agent has been waiting 15 minutes for you to approve a file write.

**The solution:** MobileCLI is a lightweight Rust daemon that runs on your dev machine and streams your AI agent terminal sessions to an iOS app. When your agent needs input — tool call approvals, confirmations, questions — you get a push notification. Read the full context on your phone and respond instantly.

**Key features:**
- Real-time terminal streaming over direct WebSocket
- Push notifications for agent input requests
- Works with Claude Code, Gemini CLI, OpenAI Codex, OpenCode
- File browser & editor for quick fixes (Pro)
- Self-hosted: LAN or Tailscale, zero cloud dependency
- ~7,000 lines of async Rust (tokio), MIT licensed

**Pricing:** Free open-source CLI daemon. Pro iOS app at $20/year or $30 lifetime.

### First Comment (Maker's Story):

Hey everyone! 👋 Maker here.

I've been using AI coding agents heavily for the past year — mostly Claude Code and Gemini CLI. They're incredible, but they have this annoying failure mode: they stop and wait for you. Tool call approvals, file write confirmations, clarifying questions. If you're not watching the terminal, the agent just... sits there.

I kept walking away from my desk — to make food, hang out with my family, go outside — and coming back to find the agent idle. I was basically chained to my desk waiting for a "y/n" prompt.

So I built MobileCLI. It's a Rust daemon that watches your terminal sessions and streams them to your phone. When the agent needs you, your phone buzzes. You glance at it, see what the agent wants, tap approve, and go back to whatever you were doing.

The thing I'm most opinionated about: no cloud. Your terminal output is sensitive — it's your code, your file paths, your environment. MobileCLI connects directly over your local network or Tailscale. Nothing goes through my servers. The daemon is fully open source (MIT).

The iOS app is paid ($20/yr or $30 lifetime) — that's how I sustain the project. It includes a file browser and editor so you can do quick fixes right from your phone.

Would love your feedback. What agents are you using? What's your workflow look like?

---

## 3. NEWSLETTER PITCH EMAILS

### 3a. TLDR Newsletter

Subject: MobileCLI: Open-source Rust daemon streams AI coding agents to your phone

Hey TLDR team,

Quick one — I built an open-source tool that solves an annoying problem with AI coding agents (Claude Code, Gemini CLI, etc.): they stop and wait for human approval, and if you're not at your desk, you lose time.

MobileCLI is a Rust daemon that streams terminal sessions to an iOS app and sends push notifications when agents need input. Approve tool calls from your phone. Fully self-hosted — direct WebSocket over LAN or Tailscale, no cloud/relay.

- CLI: MIT open source, ~7k lines async Rust
- iOS app: $20/yr or $30 lifetime
- Site: https://mobilecli.app
- GitHub: https://github.com/MobileCLI/mobilecli

Could be a good fit for TLDR or TLDR WebDev. Happy to provide any details.

Cheers,
[Name]

---

### 3b. Changelog

Subject: MobileCLI — self-hosted Rust daemon for monitoring AI agents from your phone

Hey Changelog crew,

I built something at the intersection of two things your audience cares about: Rust and AI-assisted development.

MobileCLI is a ~7,000-line async Rust daemon (tokio) that streams AI coding agent terminal sessions (Claude Code, Gemini CLI, OpenAI Codex, OpenCode) to an iOS app over direct WebSocket. When an agent needs human input, you get a push notification and can respond from your phone. No cloud, no accounts — LAN or Tailscale only.

The CLI is MIT-licensed and open source. The iOS app is a paid companion ($20/yr or $30 lifetime) with a file browser/editor.

I think there's an interesting story here about building developer tools in Rust, the UX challenges of AI agents that need human-in-the-loop, and the "no cloud" philosophy. Would love to chat on the pod or be featured in the newsletter.

GitHub: https://github.com/MobileCLI/mobilecli
Site: https://mobilecli.app

Best,
[Name]

---

### 3c. Ben's Bites

Subject: Tool that lets you approve AI agent actions from your phone

Hey Ben's Bites team,

Short pitch: AI coding agents (Claude Code, Gemini CLI, Codex) are great but they constantly need human approval for tool calls. If you step away from your desk, they just sit idle.

MobileCLI fixes this — it's a self-hosted daemon that streams your agent's terminal to your phone and pings you when it needs input. You approve from the couch and the agent keeps working. No cloud dependency, everything stays on your network.

Open source CLI (Rust/MIT) + paid iOS app ($20/yr).

Site: https://mobilecli.app

This fits the "AI workflow tooling" angle your readers love. Happy to provide screenshots, demo video, or whatever helps.

Thanks!
[Name]

---

### 3d. Console.dev

Subject: MobileCLI — open-source Rust CLI for streaming AI agent sessions to iOS

Hi Console team,

MobileCLI is an open-source Rust CLI tool I think fits Console's focus on interesting developer tools:

- **What:** A daemon that streams terminal sessions (especially AI coding agents) to an iOS app over direct WebSocket
- **Why:** AI agents like Claude Code stop and wait for tool call approvals. MobileCLI sends push notifications so you can approve from your phone
- **Tech:** ~7k lines async Rust (tokio), MIT licensed. No cloud — direct connection via LAN or Tailscale
- **Install:** `curl -fsSL https://mobilecli.app/install.sh | bash`

GitHub: https://github.com/MobileCLI/mobilecli
Site: https://mobilecli.app

The CLI is fully open source. There's a companion iOS app ($20/yr or $30 lifetime) with file browsing/editing.

Would love to be featured in a Console issue. Happy to answer any technical questions.

Best,
[Name]

---

### 3e. Rust Weekly (This Week in Rust)

Subject: MobileCLI — async Rust daemon for streaming terminals to iOS

Hi TWiR team,

I'd love to submit MobileCLI for This Week in Rust. It's a ~7,000-line async Rust project (tokio) that streams terminal sessions to an iOS app over WebSocket.

The primary use case is AI coding agents (Claude Code, Gemini CLI) — you get push notifications when agents need input and can approve tool calls from your phone. Direct WebSocket over LAN/Tailscale, no cloud.

Technically interesting bits:
- Async terminal session management with tokio
- WebSocket streaming with real-time PTY capture
- Zero-config LAN discovery
- MIT licensed

GitHub: https://github.com/MobileCLI/mobilecli
Crate/Install: `curl -fsSL https://mobilecli.app/install.sh | bash`

Thanks for considering!
[Name]

---

## 4. REDDIT POSTS

### 4a. r/rust

**Title:** I built a ~7k line async Rust daemon that streams AI coding agent terminals to your phone

**Body:**

Hey r/rust! I wanted to share a project I've been working on: MobileCLI.

It's an async Rust daemon (tokio) that captures terminal sessions and streams them over WebSocket to an iOS app. The main use case is AI coding agents — Claude Code, Gemini CLI, etc. — that stop and wait for tool call approvals. You get a push notification on your phone and can approve without being at your desk.

Some technical details that might interest this sub:

- ~7,000 lines of async Rust, built on tokio
- WebSocket streaming with real-time PTY capture
- Zero-config LAN discovery for connecting your phone
- Also works over Tailscale for remote access
- No cloud, no relay servers — direct connections only
- MIT licensed

I chose Rust because the daemon needs to be rock-solid (it's sitting between you and your AI agent) and lightweight (runs in the background all day). Memory usage stays under 15MB typically.

The CLI is fully open source. There's a companion iOS app (React Native/Expo) that's paid — $20/yr or $30 lifetime — which is how I fund continued development.

GitHub: https://github.com/MobileCLI/mobilecli
Site: https://mobilecli.app

Would love feedback on the code, architecture decisions, or anything else. PRs welcome!

---

### 4b. r/ClaudeAI

**Title:** I built an app that sends you push notifications when Claude Code needs approval — approve tool calls from your phone

**Body:**

I got tired of babysitting Claude Code. You know the drill: you kick off a task, walk to the kitchen, come back 10 minutes later, and Claude has been waiting for you to approve a file write the entire time.

So I built MobileCLI. It's a small daemon that runs on your dev machine and streams your Claude Code session to your iPhone. When Claude needs tool call approval, your phone buzzes. You read the context, tap approve, and Claude keeps working. All from your couch.

Works with other agents too (Gemini CLI, Codex, OpenCode), but honestly I built it because of Claude Code specifically.

Important: nothing goes through the cloud. The daemon connects to your phone directly over your local network or Tailscale. Your code and conversations stay on your machine.

- Free open-source CLI: `curl -fsSL https://mobilecli.app/install.sh | bash`
- iOS app: $20/yr or $30 lifetime
- Site: https://mobilecli.app

Anyone else have this problem? What's your current workaround?

---

### 4c. r/commandline

**Title:** MobileCLI: self-hosted daemon that streams any terminal session to your phone over WebSocket

**Body:**

I built a tool that lets you monitor and interact with terminal sessions from your phone. It runs as a lightweight daemon on your machine and streams over direct WebSocket (LAN or Tailscale).

The use case I built it for: AI coding agents (Claude Code, Gemini CLI) that need human input. But it works with any interactive terminal session — anything where you want to see what's happening and respond when you're away from your desk.

Technical details:
- Rust daemon, ~7k lines, async tokio
- Direct WebSocket — no cloud, no relay, no accounts
- Works on LAN automatically, or over Tailscale for remote
- Push notifications when the session needs input
- MIT licensed, open source

Install: `curl -fsSL https://mobilecli.app/install.sh | bash`
GitHub: https://github.com/MobileCLI/mobilecli

The companion iOS app is paid ($20/yr or $30 lifetime) — includes a file browser/editor. The daemon itself is free and open source.

---

### 4d. r/ChatGPTCoding

**Title:** Approve AI agent tool calls from your phone — built a self-hosted app for monitoring Claude Code / Gemini CLI / Codex remotely

**Body:**

The biggest bottleneck with AI coding agents isn't the AI — it's us. Every time Claude Code or Codex needs to write a file, run a command, or do anything meaningful, it stops and waits for approval. If you're not staring at the terminal, you're wasting the agent's time (and yours).

I built MobileCLI to fix this. It's a daemon that runs on your machine and streams your agent sessions to your phone. Push notification when the agent needs you. Approve from your phone. Agent keeps working.

Works with:
- Claude Code
- Gemini CLI
- OpenAI Codex
- OpenCode
- Any terminal-based AI agent

No cloud involved. Direct connection over your local network or Tailscale. Your code never leaves your machine.

Free open-source daemon + iOS app ($20/yr or $30 lifetime with file browser/editor).

Install: `curl -fsSL https://mobilecli.app/install.sh | bash`
Site: https://mobilecli.app

Curious what agents people here are using and whether this would fit your workflow.

---

### 4e. r/SideProject

**Title:** My side project: an iOS app + open source Rust daemon that lets you control AI coding agents from your phone

**Body:**

Hey! I've been working on MobileCLI for a while and wanted to share it here.

**The problem:** AI coding agents (Claude Code, Gemini CLI, etc.) constantly need human approval. If you walk away from your desk, the agent sits idle until you come back.

**What I built:** A Rust daemon that streams your terminal to an iOS app. Push notifications when agents need input. Approve from your phone.

**Tech stack:**
- Backend: ~7k lines of async Rust (tokio), MIT licensed
- iOS app: React Native / Expo
- Connection: Direct WebSocket over LAN or Tailscale (no cloud)

**Business model:**
- CLI daemon: Free, open source (MIT)
- iOS app: $20/yr or $30 lifetime
- Currently getting ~200 impressions/day on the App Store organically

**What I learned:**
Building a paid companion app for an open-source CLI is an interesting model. The open source part builds trust (people can verify no data leaves their network), and the app provides enough value to justify paying for.

Site: https://mobilecli.app
GitHub: https://github.com/MobileCLI/mobilecli

Would love feedback on the product, the pricing, or the business model. AMA!

---

## 5. TWEET DRAFTS (7 days, @mobilecli)

### Day 1 (Monday) — Launch / Intro Tweet
**Tweet:**
Introducing MobileCLI 🚀

Stream your AI coding agent terminal to your iPhone. Get push notifications when Claude Code / Gemini CLI needs approval. Respond from your couch.

Self-hosted. No cloud. Direct WebSocket.

Free CLI (open source) + iOS app.

https://mobilecli.app

**Media:** 30-60s demo video showing: starting daemon → agent running → push notification on phone → approving → agent continues

---

### Day 2 (Tuesday) — Problem Awareness
**Tweet:**
The biggest bottleneck with AI coding agents isn't the AI.

It's you walking to the kitchen while Claude Code waits for tool call approval.

I built @mobilecli so your phone buzzes instead of your agent sitting idle.

**Media:** Screenshot of a Claude Code terminal stuck on "Approve? (y/n)" with a timestamp showing it's been waiting

---

### Day 3 (Wednesday) — Demo / How It Works
**Tweet:**
How MobileCLI works:

1. Install the daemon (one curl command)
2. Start your AI agent as usual
3. Your phone shows the terminal in real-time
4. Agent needs approval → push notification
5. Tap approve → agent keeps going

No accounts. No cloud. Just a WebSocket on your LAN.

**Media:** 4-panel screenshot sequence showing each step on the phone

---

### Day 4 (Thursday) — Rust / Technical Angle
**Tweet:**
~7,000 lines of async Rust powering MobileCLI.

Why Rust? This daemon runs in your background all day, watching your terminals. It needs to be:
- Rock solid (no crashes)
- Lightweight (<15MB RAM)
- Fast (real-time streaming)

MIT licensed. PRs welcome.

github.com/MobileCLI/mobilecli

**Media:** Screenshot of the GitHub repo / code snippet showing the tokio WebSocket handler

---

### Day 5 (Friday) — Privacy / No Cloud Angle
**Tweet:**
Your terminal output contains:
- Source code
- File paths
- API keys (sometimes)
- Environment variables

MobileCLI never sends any of it to the cloud. Direct WebSocket between your machine and your phone. LAN or Tailscale. That's it.

https://mobilecli.app

**Media:** Simple diagram showing direct connection: Computer ↔ Phone (no cloud in between)

---

### Day 6 (Saturday) — Engagement / Weekend Casual
**Tweet:**
Saturday morning coding from the couch.

Claude Code is refactoring a module on my desktop. I'm approving tool calls from my phone while drinking coffee.

This is the workflow I built @mobilecli for.

What are you building this weekend?

**Media:** Casual photo/screenshot of phone showing MobileCLI with coffee in background (lifestyle shot)

---

### Day 7 (Sunday) — Recap / CTA
**Tweet:**
First week of @mobilecli in the wild. Quick recap:

✅ Open source Rust daemon (MIT)
✅ Works with Claude Code, Gemini CLI, Codex, OpenCode
✅ Push notifications for agent approvals
✅ No cloud, self-hosted
✅ iOS app: $20/yr or $30 lifetime

Try it:
curl -fsSL https://mobilecli.app/install.sh | bash

**Media:** App Store screenshots or product hero image

---

## BONUS: SHORT-FORM HOOK TWEETS (for replies/threads)

**Hook 1:** "AI agents are only as fast as the human approving their tool calls."

**Hook 2:** "I automated my coding but I still had to babysit the automation."

**Hook 3:** "The best developer tool I built this year is one that lets me leave my desk."

**Hook 4:** "No cloud. No accounts. No relay servers. Just a WebSocket and your phone."

---

*End of launch assets.*
