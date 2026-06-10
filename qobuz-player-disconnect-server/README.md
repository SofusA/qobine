# Disconnect server

This module includes a server implementation supporting the "Disconnect" feature of the Qobine players.

In short, this groups "devices" or streams based on a secret password. It then distributes events to each stream in a group.

The server will stream ui events and controls events, to either active or inactive devices.
