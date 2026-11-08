# VNSYNC

VNSYNC is a tool for syncing visual novel files between different devices.

## Features

- Syncing files between devices
- Easy to use interface
- Fast and efficient way to sync your visual novel save files
- Supports multiple visual novel game save file formats
- Cross-platform compatibility

## Plan

Thinking about the different plans that VNSYNC could offer:

+ Pricing
  - Free plan : Will syncing for set amount of time (1min, 5min, 10min, 30min, 1hr, 2hr, 12hr, 24hr) per save, 25mb file storage, File expire up to 30 days (cannot disable), 7 days file retention, and connect up to 4 devices (same account)
  - Premium plan : Will syncing up to custom amount of time per save, 100mb file storage, File expire up to 180 days (can be disable), 30 days file retention, and connect up to 12 devices (same account)
  - GameDev plan : Advanced features, 5Gb+ file storage or bring your own S3, API access, unlimited devices, and 180 days file retention
  - Self-hosted plan : All features, unlimited file storage, unlimited devices, file expired up to infinite time (can be disabled) and unlimited file retention (If user connects to the self-hosted [Cloud or Local] version)

## Frontend

## Backend

+ Route
  - /api/v1/sync/:id
  - /api/v1/sync/:id/status
  - /api/v1/sync/:id/download
  - /api/v1/sync/:id/upload
  - /api/v1/user
  - /api/v1/user/session
  - /api/v1/auth/signin
  - /api/v1/auth/signout
  - /api/v1/auth/callback
