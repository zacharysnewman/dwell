// Moves through the main menu (ui/menuNav.ts): past the logo screen when it shows (once a
// session), then to a page from the home.
import type { Page } from '@playwright/test';

export type MenuPage = 'home' | 'worlds' | 'new-world' | 'online' | 'versions';

const OPENER: Record<Exclude<MenuPage, 'home' | 'new-world'>, string> = {
  worlds: '#home-worlds',
  online: '#home-online',
  versions: '#home-versions',
};

/** Opens a page of the main menu (the menu must be up). */
export async function openMenuPage(page: Page, target: MenuPage): Promise<void> {
  const menu = page.locator('#main-menu');
  await menu.waitFor();
  // The view the menu shows: `logo`, `home` or a page.
  const view = async () => (await menu.getAttribute('data-view')) ?? '';
  if ((await view()) === 'logo') await page.locator('#menu-logo').click();
  // Back to the home from any page.
  for (let i = 0; i < 3 && (await view()) !== 'home'; i++) await page.keyboard.press('Escape');
  if (target === 'home') return;
  if (target === 'new-world') {
    await page.locator(OPENER.worlds).click();
    await page.locator('#new-world').click();
    return;
  }
  await page.locator(OPENER[target]).click();
}
