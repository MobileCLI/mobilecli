#!/usr/bin/env python3
"""
MobileCLI Social Media Automation Pipeline

This script monitors the blog RSS feed for new posts and auto-generates
+ posts content to Bluesky, Twitter/X, and drafts Reddit posts.

Usage:
  python social_poster.py post-blog          # Check RSS, post new blog entries
  python social_poster.py post-custom "text" # Post custom text to all platforms  
  python social_poster.py post-custom "text" --media image.png
  python social_poster.py generate "topic"   # AI-generate a post about a topic
  python social_poster.py status             # Check what's been posted
"""

import os
import sys
import json
import hashlib
import argparse
import xml.etree.ElementTree as ET
from pathlib import Path
from datetime import datetime, timezone

# ─── Config ──────────────────────────────────────────────────────────

SCRIPT_DIR = Path(__file__).parent
CONFIG_FILE = SCRIPT_DIR / "config.env"
POSTED_LOG = SCRIPT_DIR / "posted.json"
DRAFTS_DIR = SCRIPT_DIR / "drafts"
RSS_URL = "https://mobilecli.app/rss.xml"

SITE_URL = "https://mobilecli.app"
GITHUB_URL = "https://github.com/MobileCLI/mobilecli"
APP_STORE_URL = "https://apps.apple.com/us/app/mobilecli/id6757689455"


def load_config():
    """Load config from config.env file."""
    config = {}
    if CONFIG_FILE.exists():
        for line in CONFIG_FILE.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, _, value = line.partition("=")
                config[key.strip()] = value.strip()
    # Also check environment variables (override file)
    for key in [
        "BLUESKY_HANDLE", "BLUESKY_APP_PASSWORD",
        "TWITTER_CONSUMER_KEY", "TWITTER_CONSUMER_SECRET",
        "TWITTER_ACCESS_TOKEN", "TWITTER_ACCESS_TOKEN_SECRET",
        "TWITTER_HAS_MEDIA_ACCESS",
        "REDDIT_CLIENT_ID", "REDDIT_CLIENT_SECRET",
        "REDDIT_USERNAME", "REDDIT_PASSWORD",
        "OPENAI_API_KEY",
    ]:
        if os.environ.get(key):
            config[key] = os.environ[key]
    return config


def load_posted():
    """Load log of already-posted items."""
    if POSTED_LOG.exists():
        return json.loads(POSTED_LOG.read_text())
    return {"posts": []}


def save_posted(data):
    """Save posted items log."""
    POSTED_LOG.write_text(json.dumps(data, indent=2))


def get_post_hash(url):
    """Generate hash for deduplication."""
    return hashlib.md5(url.encode()).hexdigest()[:12]


# ─── RSS Parsing ─────────────────────────────────────────────────────

def fetch_rss():
    """Fetch and parse the blog RSS feed."""
    import urllib.request
    try:
        req = urllib.request.Request(RSS_URL, headers={"User-Agent": "MobileCLI-Bot/1.0"})
        with urllib.request.urlopen(req, timeout=15) as resp:
            xml_data = resp.read().decode()
        root = ET.fromstring(xml_data)
        items = []
        for item in root.iter("item"):
            title = item.find("title")
            link = item.find("link")
            desc = item.find("description")
            pub_date = item.find("pubDate")
            items.append({
                "title": title.text if title is not None else "",
                "url": link.text if link is not None else "",
                "description": desc.text if desc is not None else "",
                "date": pub_date.text if pub_date is not None else "",
            })
        return items
    except Exception as e:
        print(f"  [!] Failed to fetch RSS: {e}")
        return []


# ─── AI Content Generation ──────────────────────────────────────────

def ai_generate_posts(title, url, description, config):
    """Use OpenAI to generate platform-specific posts from blog content."""
    api_key = config.get("OPENAI_API_KEY", "")
    if not api_key:
        # Fallback: generate simple posts without AI
        return generate_simple_posts(title, url, description)

    import urllib.request
    prompt = f"""Generate social media posts for a new blog article from MobileCLI 
(a self-hosted Rust daemon + iOS app that streams AI coding agent terminals to your phone).

Blog post: "{title}"
URL: {url}
Description: {description}

Generate EXACTLY this JSON (no markdown, no code fences):
{{
  "twitter": "Tweet under 260 chars. Punchy, dev-focused. Include the URL. No hashtags.",
  "bluesky": "Bluesky post under 280 chars. Casual dev tone. Include the URL.",
  "reddit_title": "Reddit post title. Interesting, not clickbait.",
  "reddit_body": "Reddit post body, 2-3 paragraphs. Genuine, not salesy. Mention the blog post link naturally."
}}"""

    req_data = json.dumps({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.7,
        "max_tokens": 500,
    }).encode()

    req = urllib.request.Request(
        "https://api.openai.com/v1/chat/completions",
        data=req_data,
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            result = json.loads(resp.read().decode())
        content = result["choices"][0]["message"]["content"]
        # Strip markdown code fences if present
        content = content.strip()
        if content.startswith("```"):
            content = content.split("\n", 1)[1]
        if content.endswith("```"):
            content = content.rsplit("```", 1)[0]
        return json.loads(content.strip())
    except Exception as e:
        print(f"  [!] AI generation failed: {e}, using simple templates")
        return generate_simple_posts(title, url, description)


def generate_simple_posts(title, url, description):
    """Fallback: generate posts from templates without AI."""
    return {
        "twitter": f"New on the MobileCLI blog: {title}\n\n{url}",
        "bluesky": f"New post: {title}\n\n{description[:100]}...\n\n{url}",
        "reddit_title": title,
        "reddit_body": f"{description}\n\nRead more: {url}",
    }


# ─── Platform Posters ───────────────────────────────────────────────

def post_bluesky(text, config, media_path=None):
    """Post to Bluesky via AT Protocol."""
    handle = config.get("BLUESKY_HANDLE", "")
    password = config.get("BLUESKY_APP_PASSWORD", "")
    if not handle or not password:
        print("  [skip] Bluesky: no credentials configured")
        return False

    try:
        from atproto import Client
        client = Client()
        client.login(handle, password)

        if media_path and os.path.exists(media_path):
            with open(media_path, "rb") as f:
                img_data = f.read()
            # Determine if image or not
            ext = Path(media_path).suffix.lower()
            if ext in (".png", ".jpg", ".jpeg", ".gif", ".webp"):
                client.send_image(text=text, image=img_data, image_alt="MobileCLI")
            else:
                # For video, just post text (Bluesky video support is limited)
                client.send_post(text=text)
        else:
            client.send_post(text=text)
        print("  [✓] Bluesky: posted successfully")
        return True
    except Exception as e:
        print(f"  [✗] Bluesky: {e}")
        return False


def post_twitter(text, config, media_path=None):
    """Post to Twitter/X via API."""
    keys = [
        config.get("TWITTER_CONSUMER_KEY", ""),
        config.get("TWITTER_CONSUMER_SECRET", ""),
        config.get("TWITTER_ACCESS_TOKEN", ""),
        config.get("TWITTER_ACCESS_TOKEN_SECRET", ""),
    ]
    if not all(keys):
        print("  [skip] Twitter: no credentials configured")
        return False

    try:
        import tweepy

        # v2 client for posting
        client = tweepy.Client(
            consumer_key=keys[0],
            consumer_secret=keys[1],
            access_token=keys[2],
            access_token_secret=keys[3],
        )

        media_ids = None
        has_media = config.get("TWITTER_HAS_MEDIA_ACCESS", "false").lower() == "true"

        if media_path and os.path.exists(media_path) and has_media:
            # v1.1 API for media upload (requires Basic tier)
            auth = tweepy.OAuth1UserHandler(keys[0], keys[1], keys[2], keys[3])
            api_v1 = tweepy.API(auth)
            media = api_v1.media_upload(media_path)
            media_ids = [media.media_id]
            print(f"  [i] Twitter: uploaded media {media_path}")

        kwargs = {"text": text}
        if media_ids:
            kwargs["media_ids"] = media_ids

        client.create_tweet(**kwargs)
        print("  [✓] Twitter: posted successfully")
        return True
    except Exception as e:
        print(f"  [✗] Twitter: {e}")
        return False


def draft_reddit(title, body, config, subreddit="SideProject"):
    """Save a Reddit post draft for manual review."""
    DRAFTS_DIR.mkdir(exist_ok=True)
    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    draft_file = DRAFTS_DIR / f"reddit-{subreddit}-{timestamp}.md"
    draft_file.write_text(
        f"# Reddit Draft: r/{subreddit}\n\n"
        f"**Title:** {title}\n\n"
        f"**Body:**\n\n{body}\n\n"
        f"---\n"
        f"*Generated: {datetime.now().isoformat()}*\n"
        f"*Review and post manually at: https://reddit.com/r/{subreddit}/submit*\n"
    )
    print(f"  [✓] Reddit: draft saved to {draft_file}")
    return True


def post_reddit(title, body, config, subreddit="SideProject"):
    """Post to Reddit via PRAW (if configured) or save as draft."""
    client_id = config.get("REDDIT_CLIENT_ID", "")
    client_secret = config.get("REDDIT_CLIENT_SECRET", "")
    username = config.get("REDDIT_USERNAME", "")
    password = config.get("REDDIT_PASSWORD", "")

    if not all([client_id, client_secret, username, password]):
        return draft_reddit(title, body, config, subreddit)

    try:
        import praw
        reddit = praw.Reddit(
            client_id=client_id,
            client_secret=client_secret,
            username=username,
            password=password,
            user_agent="MobileCLI-Social-Bot/1.0",
        )
        sub = reddit.subreddit(subreddit)
        sub.submit(title, selftext=body)
        print(f"  [✓] Reddit r/{subreddit}: posted successfully")
        return True
    except ImportError:
        print("  [!] Reddit: praw not installed. pip install praw")
        return draft_reddit(title, body, config, subreddit)
    except Exception as e:
        print(f"  [✗] Reddit r/{subreddit}: {e}")
        return draft_reddit(title, body, config, subreddit)


# ─── Commands ────────────────────────────────────────────────────────

def cmd_post_blog(config):
    """Check RSS for new posts and share them."""
    print("Checking RSS feed for new blog posts...")
    items = fetch_rss()
    if not items:
        print("No items found in RSS feed.")
        return

    posted = load_posted()
    posted_hashes = {p["hash"] for p in posted["posts"]}
    new_items = [i for i in items if get_post_hash(i["url"]) not in posted_hashes]

    if not new_items:
        print("No new posts to share.")
        return

    print(f"Found {len(new_items)} new post(s) to share:\n")

    for item in new_items:
        print(f"━━━ {item['title']} ━━━")
        print(f"    {item['url']}\n")

        # Generate platform-specific content
        posts = ai_generate_posts(item["title"], item["url"], item["description"], config)

        # Post to each platform
        results = {}
        results["bluesky"] = post_bluesky(posts["bluesky"], config)
        results["twitter"] = post_twitter(posts["twitter"], config)
        results["reddit"] = draft_reddit(
            posts["reddit_title"], posts["reddit_body"], config
        )

        # Log it
        posted["posts"].append({
            "hash": get_post_hash(item["url"]),
            "title": item["title"],
            "url": item["url"],
            "posted_at": datetime.now(timezone.utc).isoformat(),
            "results": {k: "success" if v else "failed" for k, v in results.items()},
        })
        save_posted(posted)
        print()


def cmd_post_custom(text, config, media_path=None):
    """Post custom text to all platforms."""
    print(f"Posting to all platforms...\n")
    print(f"  Text: {text[:100]}{'...' if len(text) > 100 else ''}\n")

    post_bluesky(text, config, media_path)
    post_twitter(text, config, media_path)
    print()


def cmd_generate(topic, config):
    """AI-generate a social post about a topic."""
    api_key = config.get("OPENAI_API_KEY", "")
    if not api_key:
        print("OPENAI_API_KEY not configured. Set it in config.env")
        return

    import urllib.request
    prompt = f"""Generate a tweet for @mobilecli (a self-hosted Rust daemon + iOS app 
that streams AI coding agent terminals to your phone).

Topic: {topic}

Requirements:
- Under 260 characters
- Developer audience
- Authentic indie maker voice
- No hashtags unless very relevant

Return ONLY the tweet text, nothing else."""

    req_data = json.dumps({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.8,
        "max_tokens": 200,
    }).encode()

    req = urllib.request.Request(
        "https://api.openai.com/v1/chat/completions",
        data=req_data,
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            result = json.loads(resp.read().decode())
        tweet = result["choices"][0]["message"]["content"].strip().strip('"')
        print(f"Generated post:\n\n  {tweet}\n")
        print(f"({len(tweet)} chars)")

        confirm = input("\nPost this? (b=bluesky, t=twitter, bt=both, n=no): ").strip().lower()
        if "b" in confirm:
            post_bluesky(tweet, config)
        if "t" in confirm:
            post_twitter(tweet, config)
        if confirm == "n" or not confirm:
            print("Skipped.")
    except Exception as e:
        print(f"Failed: {e}")


def cmd_status():
    """Show posting history."""
    posted = load_posted()
    if not posted["posts"]:
        print("No posts recorded yet.")
        return

    print(f"Posted {len(posted['posts'])} item(s):\n")
    for p in posted["posts"][-10:]:
        print(f"  {p['posted_at'][:10]}  {p['title'][:60]}")
        for platform, status in p.get("results", {}).items():
            icon = "✓" if status == "success" else "✗"
            print(f"    [{icon}] {platform}")
        print()


# ─── Main ────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="MobileCLI Social Media Automation")
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("post-blog", help="Check RSS for new posts and share them")

    custom = sub.add_parser("post-custom", help="Post custom text to all platforms")
    custom.add_argument("text", help="Text to post")
    custom.add_argument("--media", help="Path to media file to attach")

    gen = sub.add_parser("generate", help="AI-generate a post about a topic")
    gen.add_argument("topic", help="Topic to generate about")

    sub.add_parser("status", help="Show posting history")

    args = parser.parse_args()
    config = load_config()

    if args.command == "post-blog":
        cmd_post_blog(config)
    elif args.command == "post-custom":
        cmd_post_custom(args.text, config, getattr(args, "media", None))
    elif args.command == "generate":
        cmd_generate(args.topic, config)
    elif args.command == "status":
        cmd_status()
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
