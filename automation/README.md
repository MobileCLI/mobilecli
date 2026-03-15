# MobileCLI Social Media Automation

Fully automated content pipeline: blog → AI-generated posts → multi-platform posting.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    CONTENT PIPELINE                              │
│                                                                  │
│  Weekly Blog Cron ──► New .md file ──► git push ──► Vercel       │
│  (Hermes agent)       (SEO-targeted)                deploy       │
│                                                       │          │
│                                                       ▼          │
│  Daily Social Cron ──► Check RSS ──► AI Generate ──► Post        │
│  (social_poster.py)    feed          platform-       │ │ │       │
│                                      specific text   │ │ │       │
│                                                      │ │ │       │
│                                          ┌───────────┘ │ │       │
│                                          ▼             │ │       │
│                                      Bluesky (free)    │ │       │
│                                                        ▼ │       │
│                                               Twitter/X  │       │
│                                            ($0-100/mo)   │       │
│                                                          ▼       │
│                                                   Reddit drafts  │
│                                                  (manual review) │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│                    VIDEO PIPELINE (optional)                      │
│                                                                   │
│  Android Emulator ──► Maestro Flow ──► Screen Record ──► FFmpeg   │
│  (headless Linux)     (YAML script)    (adb)             │ │ │    │
│                                                          │ │ │    │
│                                              Square 1:1 ─┘ │ │   │
│                                              Wide 16:9 ────┘ │   │
│                                              GIF ────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

## Quick Start

### 1. Configure credentials

```bash
cp config.env.example config.env
# Edit config.env with your credentials
```

**Minimum viable setup (free):**
- Bluesky: Create app password at bsky.app > Settings > App Passwords
- That's it. Bluesky is completely free to automate.

**Full setup:**
- Bluesky: Free (app password)
- Twitter/X: Free tier = text-only (1500/mo). Basic tier ($100/mo) = with media
- Reddit: Free API via PRAW, but semi-automated (drafts for manual review)
- OpenAI: For AI content generation ($0.01/post roughly)

### 2. Test it

```bash
# Check for new blog posts and generate social content
python3 social_poster.py post-blog

# Post custom text to all platforms
python3 social_poster.py post-custom "MobileCLI now supports Aider! Stream AI pair programming to your phone."

# Post with media
python3 social_poster.py post-custom "Demo time 🚀" --media ../promo-video/out/promo-v3-square.mp4

# AI-generate a post about a topic
python3 social_poster.py generate "why AI coding agents need mobile monitoring"

# Check posting history
python3 social_poster.py status
```

### 3. Automation (already set up)

Two Hermes cron jobs are running:

| Job | Schedule | What it does |
|-----|----------|--------------|
| Weekly Blog Post | Every 7 days | AI writes SEO blog post → git push → Vercel deploys |
| Blog-to-Social | Every 24 hours | Checks RSS → AI generates posts → posts to Bluesky/Twitter |

The pipeline is: blog cron writes post → Vercel deploys → RSS updates → social cron picks it up → posts everywhere.

## Video Generation (Optional)

For automated demo video creation on Linux using Android emulator:

```bash
# Install dependencies
./video_generator.sh setup

# Edit the Maestro flow to match your app
vim maestro_flows/demo_flow.yaml

# Record a demo
./video_generator.sh record

# Generate social-ready formats
./video_generator.sh process
```

**Requirements:** Android SDK, emulator, adb, Maestro, ffmpeg

**Note:** iOS Simulator does NOT run on Linux. For iOS-specific footage, use a macOS CI runner (GitHub Actions) or record manually on a physical device.

**Output formats:**
- `demo-square.mp4` — 1080x1080 for Twitter/Instagram
- `demo-portrait.mp4` — 1080x1920 for TikTok/Stories  
- `demo-wide.mp4` — 1920x1080 for YouTube/LinkedIn
- `demo.gif` — 480px for GitHub README

## Platform Details

### Bluesky (RECOMMENDED — $0)
- Completely free and open API (AT Protocol)
- No approval process, no rate limit tiers
- Growing dev community
- Supports images (video support limited)
- Just create an app password and go

### Twitter/X
- **Free tier ($0):** 1,500 tweets/month, text-only (NO media upload)
- **Basic tier ($100/mo):** 3,000 tweets/month, media upload supported
- Video must be MP4 H.264, max 140 seconds
- Uses tweepy (v1.1 for media, v2 for posting)

### Reddit  
- Free API, no paid tiers
- BUT: subreddits heavily restrict bot/promotional posting
- This tool saves drafts for manual review by default
- If PRAW credentials are configured, can auto-post (use cautiously)
- Best subreddits: r/SideProject, r/commandline, r/rust, r/ClaudeAI

## Cost Summary

| Component | Cost | What you get |
|-----------|------|--------------|
| Bluesky API | $0 | Full posting with images |
| Twitter Free | $0 | Text-only tweets (1500/mo) |
| Twitter Basic | $100/mo | Tweets with media |
| Reddit API | $0 | Full access (be careful with bots) |
| OpenAI (gpt-4o-mini) | ~$0.01/post | AI content generation |
| Android Emulator | $0 | Demo video recording |
| Maestro | $0 | Scripted app interactions |
| FFmpeg | $0 | Video processing |

**Minimum cost for full automation: $0/month** (Bluesky + Reddit drafts + template posts)
**Recommended: ~$1/month** (add OpenAI for AI-generated posts)
**Full power: $101/month** (add Twitter Basic for media tweets)
