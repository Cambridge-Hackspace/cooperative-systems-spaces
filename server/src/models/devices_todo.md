# Devices

This project will support "devices" for spaces that implement either control or informational services running on local space networks. There are two types
- Edge Devices will listen for MQTT messages from the Space Network and forward them to the Space Server, as well as recieve commands from the Space Server over the same link.
- Kiosk Devices will be used to display the Space Information on a mounted kiosk display device, such as calendar, weather, etc.

These devices communicate with the Space Server via MQTT, apart from registration which happens over REST endppints.

## Registration

### Server 

On our server component, I'd like to add registration workflows based on tables defined by the structs in devices.rs.

The Server Component will generate a SpaceDeviceAuthRequest which contains a code and an expiry date.  The device code is composed of 8 emojis and is only usable once.
Devices will be registered with the Space Server using the code + instance URL.

Upon registration we will create a SpaceDeviceAuth instance for the new SpaceDevice for it to authenticate with the Space Server and return this information to the device.

### Edge

Our edge should allow registration via either a web ui or a subcommand in clap where you pass in the instance URL and the device code.

## Management

There should be API endpoints for generating new device invites codes, listing devices, deleting devices, and renaming devices and whatever else is necessary to support this functionality overall.

### Naming

Devices will generate friendly names at startup, and pass this name to the Space Server upon registration. The space server will store this name in the database. 
When this name is updated on the server, we should emit an mqtt message over /devices/<id>/name and the client should update it's local config file and reload itself

### Last Seen

Devices when registered will send a heartbeat message to the Space Server every 3 minutes over /devices/<id>/heartbeat.  The Space Server should have a listerner that will update the last_seen_at field for the device when these are recieved.

### Other Fields

These fields should be published by the device at startup and then every 15 minutes, over /devices/<id>/data. The Space Server should have a listerner that will update the fields for the device when these are recieved.
```
pub mac_address: String,
pub software_version: String,
pub ipv4_address: Option<String>,
pub ipv6_address: Option<String>,
pub uptime: usize
pub platform: SpaceDevicePlatform
```

## Edge UI (Other)

Once registration is complete, Edge UI should have no information except for the version at this time. We should embed a new Vue frontend that will allow the user to gregister the device and display the post-registration information.

## Server UI

The server UI should implement controls for devices under a new admin section.  This should include:
- Generating a new device code
- Expiring device codes
- Viewing device code history
- Listing devices and showing their last seen time
- Renaming a device
- Deleting device