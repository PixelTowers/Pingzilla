# PingZilla TODO

## App Store Submission Checklist

### Pre-Submission (Manual Steps)

- [ ] Get Apple Team ID from [developer.apple.com/account](https://developer.apple.com/account)
- [ ] Update `src-tauri/Entitlements.plist` - Replace `YOUR_TEAM_ID` with actual Team ID
- [ ] Create App ID in Apple Developer portal (Bundle ID: `com.pingzilla.monitor`)
- [ ] Create "Apple Distribution" certificate
- [ ] Create "Mac App Store Connect" provisioning profile
- [ ] Download `.provisionprofile` and place in `src-tauri/`
- [ ] Create app record in [App Store Connect](https://appstoreconnect.apple.com)
- [ ] Set up banking/tax info in App Store Connect
- [ ] Create privacy policy (host on GitHub Pages or similar)

### Build & Sign

- [ ] Build universal binary: `make universal`
- [ ] Sign the app with Apple Distribution certificate
- [ ] Create installer package: `make pkg`
- [ ] Upload to App Store Connect: `make upload`

### App Store Metadata

- [ ] App Name: PingZilla
- [ ] Subtitle: Network Ping Monitor
- [ ] Category: Utilities
- [ ] Price: $1.99 - $2.99 (your choice)
- [ ] Screenshots (1280x800 or 1440x900):
  - [ ] Menu bar with icon
  - [ ] Popup window with graph
  - [ ] Settings panel
  - [ ] Statistics view

### App Description

```
PingZilla is a lightweight menu bar app that monitors your network latency in real-time.

Features:
• Real-time ping monitoring in your menu bar
• Monitor multiple targets simultaneously
• Visual latency graph (last 2 minutes)
• Statistics dashboard (min/max/avg, packet loss)
• High latency notifications
• Cute Godzilla icons that change based on connection quality
• Launch at login support
• 24-hour history persistence

Perfect for developers, gamers, and anyone who wants to keep an eye on their network connection.
```

### Keywords (max 100 chars)

`ping,network,latency,monitor,menu bar,connection,internet,speed,utility`

---

## Future Features

- [ ] Traceroute on-demand
- [ ] Export history to CSV/JSON
- [ ] Custom notification sounds
- [ ] Widget support
- [ ] iOS companion app
