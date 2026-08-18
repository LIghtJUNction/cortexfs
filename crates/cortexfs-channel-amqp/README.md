# cortexfs-channel-amqp

Process-isolated AMQP channel driver for CortexFS. It consumes bounded text
messages from a RabbitMQ-compatible exchange/queue, routes them through the
canonical `cortexfs.channel.socket/v1` ABI, publishes the text reply, and only
acknowledges a delivery after the runtime and publish path succeed.

The runtime-facing driver is configured separately from broker credentials:

```text
CORTEXFS_AMQP_URL=amqps://user:password@broker/vhost
CORTEXFS_AMQP_EXCHANGE=agent.events
CORTEXFS_AMQP_QUEUE=cortexfs-coder
CORTEXFS_AMQP_ROUTING_KEYS=agent.coder
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/amqp.sock
```
