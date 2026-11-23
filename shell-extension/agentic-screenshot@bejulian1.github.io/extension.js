// Stille opptak for Agentic Screenshot. Portalen ber alltid GNOME om hvit blits og
// lukkerlyd, og svarer først når blitsen er ferdig. Her tar vi bildet direkte, og
// holder hurtigtasten selv, så den virker i alle apper uten at Agentic Screenshot må kjøre.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Flashspot} from 'resource:///org/gnome/shell/ui/screenshot.js';
import {DBusSenderChecker} from 'resource:///org/gnome/shell/misc/util.js';
import {Extension, InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';

const APP_ID = 'io.github.AgenticScreenshot';
const BUS_NAME = `${APP_ID}.Shell`;
const OBJECT_PATH = '/io/github/AgenticScreenshot/Shell';
const DEFAULT_ACCEL = '<Alt>k';
// Overlegget skal rekke å dekke skjermen før Agentic Screenshots egne vinduer vises igjen.
const WINDOWS_BACK_AFTER_MS = 250;
// Sikkerhetsnett hvis skjermen ikke tegner, for eksempel når den er av.
const REDRAW_TIMEOUT_MS = 100;
// Holder man tasten inne, skal den ikke starte Agentic Screenshot om og om igjen.
const REPEAT_GUARD_US = 300_000;
const IFACE = `<node>
  <interface name="${BUS_NAME}">
    <method name="Capture">
      <arg type="s" direction="in" name="path"/>
    </method>
    <method name="Shutter">
      <arg type="b" direction="in" name="flash"/>
      <arg type="b" direction="in" name="sound"/>
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
      <arg type="i" direction="in" name="width"/>
      <arg type="i" direction="in" name="height"/>
    </method>
    <method name="BindShortcut">
      <arg type="s" direction="in" name="accel"/>
    </method>
  </interface>
</node>`;

function shortcutFile(dir = 'agentic-screenshot') {
    return Gio.File.new_for_path(
        GLib.build_filenamev([GLib.get_user_config_dir(), dir, 'shell-shortcut']));
}

// Før appen har flyttet filene fra 1.x, ligger tasten fortsatt under det gamle navnet.
function savedAccel() {
    for (const file of [shortcutFile(), shortcutFile('klipp')]) {
        try {
            const [, bytes] = file.load_contents(null);
            return new TextDecoder().decode(bytes).trim();
        } catch {
        }
    }
    return DEFAULT_ACCEL;
}

class Service {
    constructor() {
        // Samme vern som GNOMEs egen tjeneste: bare navngitte avsendere slipper inn.
        this._checker = new DBusSenderChecker([APP_ID]);
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
        this._nameId = Gio.DBus.session.own_name(BUS_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
        this._action = Meta.KeyBindingAction.NONE;
        this._fired = 0;
        this._hidden = [];
        global.display.connectObject('accelerator-activated', (_display, action) => {
            if (action === this._action)
                this._launch();
        }, this);
        // Står brukeren i en annen app, ville overlegget åpnet uten fokus. Da virker
        // verken Esc eller kopiering, for Wayland krever fokus for begge.
        global.window_manager.connectObject('map', (_wm, actor) => {
            const window = actor.metaWindow;
            if (window.get_wm_class() === APP_ID && window.is_fullscreen())
                Main.activateWindow(window);
        }, this);
        this._grab(savedAccel());
    }

    destroy() {
        global.display.disconnectObject(this);
        global.window_manager.disconnectObject(this);
        Main.overview.disconnectObject(this);
        this._grab('');
        this._showWindows();
        Gio.DBus.session.unown_name(this._nameId);
        this._dbus.unexport();
        this._checker.destroy();
    }

    _grab(accel) {
        if (this._action !== Meta.KeyBindingAction.NONE) {
            global.display.ungrab_accelerator(this._action);
            Main.wm.allowKeybinding(
                Meta.external_binding_name_for_action(this._action), Shell.ActionMode.NONE);
            this._action = Meta.KeyBindingAction.NONE;
        }
        if (!accel)
            return true;
        const action = global.display.grab_accelerator(accel, Meta.KeyBindingFlags.NONE);
        if (action === Meta.KeyBindingAction.NONE)
            return false;
        Main.wm.allowKeybinding(
            Meta.external_binding_name_for_action(action),
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW);
        this._action = action;
        return true;
    }

    // Starter via .desktop-handlingen, så en kjørende app får tasten på vanlig vis og
    // en stengt app starter. Handlingen gir også overlegget lov til å ta fokus.
    _launch() {
        const now = GLib.get_monotonic_time();
        if (now - this._fired < REPEAT_GUARD_US)
            return;
        this._fired = now;
        // Oversikten ligger over alle vinduer, også overlegget.
        if (Main.overview.visible) {
            Main.overview.connectObject('hidden', () => {
                Main.overview.disconnectObject(this);
                this._launchApp();
            }, this);
            Main.overview.hide();
        } else {
            this._launchApp();
        }
    }

    _launchApp() {
        Shell.AppSystem.get_default().lookup_app(`${APP_ID}.desktop`)?.get_app_info()
            .launch_action('area', global.create_app_launch_context(0, -1));
    }

    // Vinduene skjules uten å lukkes, så appen brukeren jobber i beholder fokus.
    _hideWindows() {
        this._hidden = global.get_window_actors().filter(
            a => a.metaWindow.get_wm_class() === APP_ID && a.visible);
        this._hidden.forEach(a => a.hide());
    }

    // Skjermbildet gjenbruker klippingen fra forrige tegning. Uten en ny tegning ville
    // området bak vinduene våre mangle i bildet.
    _redrawn() {
        return new Promise(resolve => {
            const finish = () => {
                global.stage.disconnect(paintId);
                GLib.source_remove(timeoutId);
                resolve();
            };
            const paintId = global.stage.connect('after-paint', finish);
            const timeoutId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, REDRAW_TIMEOUT_MS, () => {
                global.stage.disconnect(paintId);
                resolve();
                return GLib.SOURCE_REMOVE;
            });
            global.stage.queue_redraw();
        });
    }

    _showWindows() {
        if (this._showId)
            GLib.source_remove(this._showId);
        this._showId = 0;
        this._hidden.forEach(a => a.show());
        this._hidden = [];
    }

    _showWindowsSoon() {
        this._showId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, WINDOWS_BACK_AFTER_MS, () => {
            this._showId = 0;
            this._showWindows();
            return GLib.SOURCE_REMOVE;
        });
    }

    // Blits og lyd er brukerens valg, og kommer når bildet er tatt. Uten mål dekker
    // blitsen hele skrivebordet.
    async ShutterAsync([flash, sound, x, y, width, height], invocation) {
        try {
            await this._checker.checkInvocation(invocation);
            if (flash) {
                const area = width > 0 && height > 0
                    ? {x, y, width, height}
                    : {x: 0, y: 0, width: global.stage.width, height: global.stage.height};
                new Flashspot(area).fire();
            }
            if (sound) {
                global.display.get_sound_player().play_from_theme(
                    'screen-capture', 'Skjermbilde tatt', null);
            }
            invocation.return_value(null);
        } catch (e) {
            this._fail(invocation, e);
        }
    }

    async BindShortcutAsync([accel], invocation) {
        try {
            await this._checker.checkInvocation(invocation);
            if (!this._grab(accel))
                throw new GLib.Error(Gio.IOErrorEnum, Gio.IOErrorEnum.INVALID_ARGUMENT, `Opptatt: ${accel}`);
            const file = shortcutFile();
            GLib.mkdir_with_parents(file.get_parent().get_path(), 0o700);
            file.replace_contents(
                new TextEncoder().encode(accel), null, false, Gio.FileCreateFlags.NONE, null);
            invocation.return_value(null);
        } catch (e) {
            this._fail(invocation, e);
        }
    }

    _fail(invocation, e) {
        if (e instanceof GLib.Error)
            invocation.return_gerror(e);
        else
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', `${e}`);
    }

    async CaptureAsync([path], invocation) {
        let stream = null;
        try {
            await this._checker.checkInvocation(invocation);
            if (GLib.path_get_dirname(path) !== GLib.get_user_runtime_dir())
                throw new GLib.Error(Gio.IOErrorEnum, Gio.IOErrorEnum.PERMISSION_DENIED, 'Feil mappe');
            stream = Gio.File.new_for_path(path).replace(
                null, false, Gio.FileCreateFlags.PRIVATE, null);
            this._hideWindows();
            if (this._hidden.length > 0)
                await this._redrawn();
            await new Shell.Screenshot().screenshot(false, stream);
            stream.close(null);
            invocation.return_value(null);
        } catch (e) {
            stream?.close(null);
            this._fail(invocation, e);
        } finally {
            this._showWindowsSoon();
        }
    }
}

export default class AgenticScreenshotExtension extends Extension {
    enable() {
        this._service = new Service();
        // Overlegget er et fullskjermsvindu. Uten dette zoomer GNOME det opp fra
        // bunnen av skjermen og krymper det igjen, som et vanlig vindu.
        this._injections = new InjectionManager();
        this._injections.overrideMethod(Main.wm, '_shouldAnimateActor', original => {
            return function (actor, types) {
                return actor.metaWindow?.get_wm_class() !== APP_ID &&
                    original.call(this, actor, types);
            };
        });
    }

    disable() {
        this._injections.clear();
        this._injections = null;
        this._service.destroy();
        this._service = null;
    }
}
