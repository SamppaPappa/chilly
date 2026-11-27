# Privacy Policy – Chilly

*Last updated: November 27, 2026*

Chilly is a free, open-source Twitch desktop client maintained by SamppaLandia. We respect your privacy and do **not** collect, store, sell, or transmit personal data or telemetry to the developer or any third-party analytics service.

## 1. Local Storage Only

All settings, channel notification preferences, watch streak priority order, window size, OAuth tokens, and optional local diagnostic logs (`chilly.log`) are stored **strictly locally on your own computer** in your user application data folder (`%APPDATA%\fi.samppalandia.chilly`).

Your Twitch password never passes through Chilly. Login uses Twitch's official OAuth Device Code flow and Twitch's own web login page inside Microsoft Edge WebView2.

## 2. Network Connections

Chilly only makes network connections required for its core features:

1. **Twitch (`https://id.twitch.tv`, `https://api.twitch.tv`, `https://www.twitch.tv`):**
   Used for logging in, fetching your followed/live channels, watching streams and chat, searching/discovering channels, creating clips, checking moderated channels, and claiming channel points/drops. Subject to [Twitch's Privacy Notice](https://www.twitch.tv/p/legal/privacy-notice/).
2. **FrankerFaceZ (`https://cdn2.frankerfacez.com`) and third-party emote add-ons (BetterTTV / 7TV):**
   Loaded inside Twitch chat when enabled in settings to display custom chat emotes.
3. **Update Check (`https://samppalandia.wtf/chilly/latest.json`):**
   Chilly checks this static JSON file on startup and once every 24 hours to notify you when a new version is available. No personal identifiers, Twitch usernames, or telemetry are sent with this request.

## 3. Data Deletion

Logging out inside Chilly clears your OAuth tokens and WebView2 cookies. Uninstalling Chilly with the "Delete app data" option removes all local configuration and cache files from your computer.

## 4. Contact

For questions or bug reports, open an issue on [GitHub](https://github.com/SamppaPappa/chilly) or join the community Discord linked on [https://samppalandia.wtf/chilly/](https://samppalandia.wtf/chilly/).
