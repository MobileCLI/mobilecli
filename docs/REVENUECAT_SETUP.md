# RevenueCat Setup (Files Premium)

MobileCLI's "Files Premium" is enforced client-side (mobile app). The desktop daemon is self-hosted and cannot reliably validate receipts without introducing cloud state, so the goal is a clean App Store / Play billing implementation with a simple UX.

**Business model**
- Lifetime: **$30** one-time
- Subscription: **$20/year** with **3-day free trial** (eligible users)

## 1. Create Products in the Stores

1. iOS (App Store Connect)
1. Create a non-consumable IAP: `mobilecli_files_lifetime` ($30)
1. Create an auto-renewable subscription: `mobilecli_files_yearly` ($20/year)
1. Add an introductory offer / free trial (3 days) for the yearly subscription

Do the equivalent in Google Play Console for Android.

## 2. Configure RevenueCat

1. Create a RevenueCat project and add iOS + Android apps
1. Create an **entitlement** named `files` (this repo expects entitlement id `files`)
1. Create an **offering** (typically "default") that contains:
1. A **yearly** package (ANNUAL) tied to `mobilecli_files_yearly`
1. A **lifetime** package (LIFETIME) tied to `mobilecli_files_lifetime`

## 3. Add RevenueCat SDK (Expo)

RevenueCat usage is wired via dynamic import in `mobile/hooks/usePremium.ts` and requires the native SDK.

Install the native dependency in `mobile/`:

```bash
npm install react-native-purchases
```

Then run a prebuild (or EAS build) so native projects pick it up. (Expo Go will not work.)

## 4. Set API Keys (EAS / Env)

Set these env vars for your EAS build profiles (or local `.env`):

- `EXPO_PUBLIC_REVENUECAT_IOS_API_KEY`
- `EXPO_PUBLIC_REVENUECAT_ANDROID_API_KEY`

Example file: `mobile/.env.example`.

Notes:
- These are RevenueCat **public SDK keys** (`appl_...` / `goog_...`). The mobile app must not use RevenueCat secret keys.
- If you later build a backend that calls RevenueCat's REST API, use the **v2** secret key server-side only.

## 5. Verify In-App Flow

1. Build a dev client / TestFlight build
1. Open `Files` tab, hit the paywall
1. Confirm purchase updates `hasFileAccess` (entitlement `files`)
1. Confirm restore works
