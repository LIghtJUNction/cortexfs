# cortexfs-channel-mqtt

Process-isolated MQTT channel driver for CortexFS. It translates bounded
MQTT text or JSON publishes into the generic `cortexfs.channel.socket/v1`
message ABI and publishes agent replies back to the originating topic.

The broker URL, topics, credentials, and client id stay in the driver process;
they never cross the generic channel socket or enter `/ctx`.
