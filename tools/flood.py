import asyncio
from dbus_next.aio import MessageBus
from dbus_next import Variant
async def main():
    bus = await MessageBus().connect()
    intro = await bus.introspect('org.freedesktop.Notifications', '/org/freedesktop/Notifications')
    n = bus.get_proxy_object('org.freedesktop.Notifications', '/org/freedesktop/Notifications', intro).get_interface('org.freedesktop.Notifications')
    for i in range(300):
        await n.call_notify('Flood', 0, '', f'Message {i}', 'x' * (i % 400) + ' <b>bold</b> &lt;tag&gt; \u00e9\u4e2d\U0001F600', ['default', 'Go'] if i % 3 == 0 else [], {'urgency': Variant('y', i % 3)}, i % 7 * 1000 - 1)
    # bad calls must not crash the shell
    try: await n.call_close_notification(999999)
    except Exception as e: print('close err', e)
asyncio.run(main())
