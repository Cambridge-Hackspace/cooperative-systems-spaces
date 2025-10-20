# **Collaborative Systems**: _Spaces_

Welcome to your collaborative workspace management system. Manage your makerspace, hackerspace, or community workspace with ease. 

## Why?

After trying out some of the existing solutions, I decided to start from scratch taking the very best from everything I've seen.

## What?

**CS**:_S_ is a binary written in Rust with an embedded VUE frontend. 
A release build with the embedded frontend is under 20 megs. 

Almost everything is configurable via the config.toml file. At first start the system will create a default file if you have not provided one.

## Features
- Registration 
  - Can be disabled
  - Challenges with Captcha, Passphrase, and Throttling
- Calendar
  - Embed Multiple iCal instances
- Permissions System with Roles
  - Newbies (Can't do much)
  - Members
  - Staff
  - Admins
- Tool Management
  - Manage tools and their metadata
  - Change tools into states for maint and lockout, with notes
  - ToolPass Compatible APIs
- Tool Training
  - Define optional Multi-Step training regimes for a given tool, with optional expiry.
  - Assign trainers to tools
  - Record Training Sessions
  - Tool Training controls ToolPass
- User Profile Pages
  - Staff Configurable Fields with automatic validation
- CLI Tool for Administrative Tasks
- Comprehensive Auditing Capabilities
- Configuration Hot Reload

### TODO
- Link Blocks on the landing page for other sites

## Running

### Binary

Build a release binary with `cargo build --release` or download a pre-built binary from the [releases](https://github.com/neiam/cooperative-systems-spaces/releases) page.

`export DATABASE__URL=postgres://url/database`
`./css-server-linux`

### Container

`podman volume create cssconfig`
`podman pull ghcr.io/neiam/cooperative-systems-spaces/app:latest`
`podman run -p 4399:4399 -e CONFIG_PATH=/app/config/config.toml -e DATABASE__URL=postgres://url/database -v cssconfig:/app/config ghcr.io/neiam/cooperative-systems-spaces/app:latest`

## LICENSE

**Collaborative Systems**: _Spaces_ is Free Software under the AGPL

## Shoutouts

Heavily Inspired by MemberMatters and other similar systems.