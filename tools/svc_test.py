import asyncio, sys
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface, method, dbus_property, signal
from dbus_next import Variant, PropertyAccess

LOG = open('/tmp/svc_events.log', 'a')
def log(*a):
    print(*a, file=LOG, flush=True)

class SS(ServiceInterface):
    def __init__(self): super().__init__('org.freedesktop.ScreenSaver')
    @method()
    def Inhibit(self, app: 's', reason: 's') -> 'u':
        log('INHIBIT', app, reason); return 42
    @method()
    def UnInhibit(self, c: 'u'):
        log('UNINHIBIT', c)

class SNI(ServiceInterface):
    def __init__(self): super().__init__('org.kde.StatusNotifierItem')
    @dbus_property(access=PropertyAccess.READ)
    def IconName(self) -> 's': return 'folder'
    @dbus_property(access=PropertyAccess.READ)
    def Title(self) -> 's': return 'Test Indicator'
    @dbus_property(access=PropertyAccess.READ)
    def Id(self) -> 's': return 'testind'
    @dbus_property(access=PropertyAccess.READ)
    def Status(self) -> 's': return 'Active'
    @dbus_property(access=PropertyAccess.READ)
    def Menu(self) -> 'o': return '/MenuBar'
    @dbus_property(access=PropertyAccess.READ)
    def ItemIsMenu(self) -> 'b': return False
    @dbus_property(access=PropertyAccess.READ)
    def IconThemePath(self) -> 's': return ''
    @dbus_property(access=PropertyAccess.READ)
    def IconPixmap(self) -> 'a(iiay)': return []
    @method()
    def Activate(self, x: 'i', y: 'i'): log('ACTIVATE', x, y)
    @method()
    def SecondaryActivate(self, x: 'i', y: 'i'): log('SECONDARY', x, y)
    @method()
    def ContextMenu(self, x: 'i', y: 'i'): log('CONTEXTMENU', x, y)
    @method()
    def Scroll(self, d: 'i', o: 's'): log('SCROLL', d, o)

class Menu(ServiceInterface):
    def __init__(self): super().__init__('com.canonical.dbusmenu')
    @method()
    def GetLayout(self, parent: 'i', depth: 'i', props: 'as') -> 'u(ia{sv}av)':
        def item(i, label, **kw):
            d = {'label': Variant('s', label)}
            for k, v in kw.items(): d[k.replace('_','-')] = v
            return Variant('(ia{sv}av)', [i, d, []])
        sub = Variant('(ia{sv}av)', [10, {'label': Variant('s', 'More'), 'children-display': Variant('s', 'submenu')},
                                     [item(11, 'Sub item A'), item(12, 'Sub item B')]])
        kids = [item(1, 'Open Window'), item(2, 'Check me', toggle_type=Variant('s','checkmark'), toggle_state=Variant('i',1)),
                Variant('(ia{sv}av)', [3, {'type': Variant('s', 'separator')}, []]),
                item(4, 'Disabled', enabled=Variant('b', False)), sub, item(5, '_Quit')]
        return [1, [0, {'children-display': Variant('s', 'submenu')}, kids]]
    @method()
    def AboutToShow(self, i: 'i') -> 'b': return False
    @method()
    def Event(self, i: 'i', e: 's', d: 'v', t: 'u'): log('MENUEVENT', i, e)

async def main(mode):
    bus = await MessageBus().connect()
    if mode == 'services':
        bus.export('/org/freedesktop/ScreenSaver', SS())
        await bus.request_name('org.freedesktop.ScreenSaver')
        bus.export('/StatusNotifierItem', SNI())
        bus.export('/MenuBar', Menu())
        await bus.request_name('org.kde.StatusNotifierItem-1234-1')
        intro = await bus.introspect('org.kde.StatusNotifierWatcher', '/StatusNotifierWatcher')
        w = bus.get_proxy_object('org.kde.StatusNotifierWatcher', '/StatusNotifierWatcher', intro)
        iface = w.get_interface('org.kde.StatusNotifierWatcher')
        await iface.call_register_status_notifier_item('org.kde.StatusNotifierItem-1234-1')
        log('REGISTERED')
        await asyncio.sleep(3600)
    else:
        intro = await bus.introspect('org.freedesktop.Notifications', '/org/freedesktop/Notifications')
        n = bus.get_proxy_object('org.freedesktop.Notifications', '/org/freedesktop/Notifications', intro).get_interface('org.freedesktop.Notifications')
        caps = await n.call_get_capabilities(); log('CAPS', caps)
        info = await n.call_get_server_information(); log('INFO', info)
        i1 = await n.call_notify('Mail', 0, 'mail-unread', 'New message from Alice', 'Hey, are we still meeting at <b>5pm</b>? Bring the report &amp; charts.', ['default', 'Open', 'reply', 'Reply'], {'urgency': Variant('y', 1)}, 8000)
        i2 = await n.call_notify('Updater', 0, 'system-software-update', 'Updates available', '12 packages can be upgraded.', [], {}, 5000)
        log('IDS', i1, i2)

asyncio.run(main(sys.argv[1]))
