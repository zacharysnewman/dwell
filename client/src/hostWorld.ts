// Hosting the local world from the game menu (ARCHITECTURE.md §10.2, Phase 5c): wires Host… to the
// master, the hosting relay and the page's lifecycle (ADR 0009): the screen stays awake while
// hosting; a hidden page pauses the world (guests see "Host paused"); leaving the page ends it.
import type { LocalSession } from './net/connect';
import { startHosting, type HostRelay } from './net/hosting';
import { configuredMasterUrl, MasterClient } from './net/master';
import { codeLink } from './ui/launch';
import { HostPanel } from './ui/hostPanel';
import type { SettingsMenu } from './ui/settingsMenu';

/** Keeps the screen on while hosting, where the browser allows it (re-taken when shown again). */
class WakeLock {
  private lock: WakeLockSentinel | null = null;
  private wanted = false;

  set(on: boolean): void {
    this.wanted = on;
    if (on) this.take();
    else {
      void this.lock?.release().catch(() => undefined);
      this.lock = null;
    }
  }

  /** The page was shown again (browsers release the lock when it is hidden). */
  shown(): void {
    if (this.wanted) this.take();
  }

  private take(): void {
    if (!('wakeLock' in navigator) || (this.lock && !this.lock.released)) return;
    navigator.wakeLock.request('screen').then(
      (lock) => {
        if (this.wanted) this.lock = lock;
        else void lock.release();
      },
      () => undefined, // not allowed (e.g. low battery): hosting works without it
    );
  }
}

export function enableHosting(settings: SettingsMenu, local: LocalSession, mobile: boolean): void {
  const base = configuredMasterUrl();
  let relay: HostRelay | null = null;
  const wake = new WakeLock();
  const stop = () => {
    relay?.stop();
    relay = null;
    wake.set(false);
  };
  const panel: HostPanel = new HostPanel({
    mobile,
    start: async (hostSettings) => {
      if (!base) throw new Error('this build has no master server configured');
      const master = new MasterClient(base, local.key);
      const hosting = await startHosting(local.loopback, master, local.key.publicKey, hostSettings);
      relay = hosting.relay;
      relay.onGuestsChanged = (count) => {
        panel.setGuests(count);
      };
      relay.onRoomLost = () => {
        panel.say('Lost the master server: the code no longer works, but guests stay connected.');
      };
      wake.set(true);
      return { display: hosting.display, link: codeLink(location.href, hosting.code) };
    },
    stop,
  });
  settings.addHostPanel(panel.element);

  document.addEventListener('visibilitychange', () => {
    const hidden = document.visibilityState === 'hidden';
    relay?.setPaused(hidden);
    if (!hidden) wake.shown();
  });
  // Leaving the page (or quitting to the menu) ends hosting; guests are told if there is time.
  window.addEventListener('pagehide', stop);
}
