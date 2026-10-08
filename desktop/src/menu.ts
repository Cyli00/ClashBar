import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { button, el, icon, iconButton } from './ui';

export interface MenuItem {
  id: string;
  label: string;
  detail?: string;
  value?: string;
  disabled?: boolean;
  checked?: boolean;
  kind?: 'item' | 'separator';
  secondaryId?: string;
  secondaryLabel?: string;
}
export interface MenuModel { id: string; title: string; items: MenuItem[] }
export interface MenuChoice extends Omit<MenuItem, 'id' | 'secondaryId'> {
  action: () => void | Promise<unknown>;
  secondaryAction?: () => void | Promise<unknown>;
}
let sequence = 0;
let active: { model: MenuModel; anchor: HTMLElement; rect: { x: number; y: number; width: number; height: number }; callbacks: Map<string, () => void | Promise<unknown>> } | null = null;
let inline: HTMLElement | null = null;
let registered = false;
let hoverOpenTimer: ReturnType<typeof setTimeout> | undefined, hoverCloseTimer: ReturnType<typeof setTimeout> | undefined;

export async function initializeMenus() {
  if (!isTauri() || registered) return;
  registered = true;
  await listen<{ menuId: string; actionId: string }>('attached-menu-action', event => {
    if (active?.model.id !== event.payload.menuId) return;
    const callback = active.callbacks.get(event.payload.actionId);
    if (callback) void callback();
  });
  await listen<{ menuId: string }>('attached-menu-closed', event => {
    if (active?.model.id !== event.payload.menuId) return;
    active.anchor.setAttribute('aria-expanded', 'false');
    if (active.anchor.isConnected && document.hasFocus()) active.anchor.focus({ preventScroll: true });
    active = null;
  });
  await listen<{ menuId: string; hovered: boolean }>('attached-menu-hover', event => {
    if (active?.model.id !== event.payload.menuId) return;
    if (event.payload.hovered) clearTimeout(hoverCloseTimer);
    else scheduleMenuClose();
  });
}

export function menuIsOpen() { return active !== null; }
export function scheduleMenuOpen(open: () => void) { clearTimeout(hoverOpenTimer); clearTimeout(hoverCloseTimer); hoverOpenTimer = setTimeout(open, 150); }
export function scheduleMenuClose() { clearTimeout(hoverOpenTimer); clearTimeout(hoverCloseTimer); hoverCloseTimer = setTimeout(() => { void dismissMenu(); }, 100); }

export async function dismissMenu() {
  clearTimeout(hoverOpenTimer); clearTimeout(hoverCloseTimer);
  inline?.remove(); inline = null;
  if (active) {
    active.anchor.setAttribute('aria-expanded', 'false');
    active = null;
    if (isTauri()) await invoke('hide_attached_menu');
  }
}

export async function showMenu(anchor: HTMLElement, title: string, choices: MenuChoice[], focus = true) {
  await dismissMenu();
  const id = `menu-${++sequence}`;
  const callbacks = new Map<string, () => void | Promise<unknown>>();
  const items = choices.map((choice, index): MenuItem => {
    const itemId = `${id}-${index}`;
    callbacks.set(itemId, choice.action);
    const secondaryId = choice.secondaryAction ? `${itemId}-test` : undefined;
    if (secondaryId && choice.secondaryAction) callbacks.set(secondaryId, choice.secondaryAction);
    const { action: _action, secondaryAction: _secondaryAction, ...item } = choice;
    return { ...item, id: itemId, secondaryId };
  });
  const model = { id, title, items };
  const bounds = anchor.getBoundingClientRect();
  const rect = { x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height };
  active = { model, anchor, rect, callbacks }; anchor.setAttribute('aria-expanded', 'true');
  if (isTauri()) {
    await initializeMenus();
    await invoke('show_attached_menu', { anchor: rect, width: 300, height: Math.min(480, 36 + items.length * 29), menu: model, focus });
  } else {
    inline = el('div', 'attached-menu'); inline.setAttribute('role', 'dialog'); inline.setAttribute('aria-label', title);
    inline.append(renderMenu(model, itemId => { const cb = callbacks.get(itemId); if (cb) { void cb(); if (!itemId.endsWith('-test')) void dismissMenu(); } }));
    const rect = anchor.getBoundingClientRect();
    inline.style.left = `${Math.max(8, Math.min(rect.left, window.innerWidth - 308))}px`;
    inline.style.top = `${Math.max(8, Math.min(rect.bottom, window.innerHeight - Math.min(480, 36 + items.length * 29)))}px`;
    document.body.append(inline); inline.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
  }
}

export async function updateMenuValue(label: string, value: string) {
  if (!active) return;
  for (const item of active.model.items) if (item.label === label) item.value = value;
  if (isTauri()) {
    await invoke('show_attached_menu', { anchor: active.rect, width: 300, height: Math.min(480, 36 + active.model.items.length * 29), menu: active.model, focus: false });
  }
}

export function renderMenu(model: MenuModel, onAction: (id: string) => void) {
  const fragment = document.createDocumentFragment();
  fragment.append(el('div', 'menu-heading', model.title));
  const list = el('div', 'menu-body'); list.setAttribute('role', 'menu'); list.setAttribute('aria-label', model.title);
  for (const item of model.items) {
    if (item.kind === 'separator') { const separator = el('hr', 'menu-separator'); separator.setAttribute('role', 'separator'); list.append(separator); continue; }
    const row = el('div', 'menu-row');
    const select = button('', () => onAction(item.id), 'menu-node'); select.disabled = Boolean(item.disabled);
    select.setAttribute('role', item.checked === undefined ? 'menuitem' : 'menuitemradio');
    if (item.checked !== undefined) select.setAttribute('aria-checked', String(item.checked));
    const check = el('span', `menu-check ${item.checked ? 'checked' : ''}`); if (item.checked) check.append(icon('check'));
    const title = el('span', 'menu-title', item.label); title.title = item.label;
    select.append(check, title);
    if (item.detail) select.append(el('span', 'node-type', item.detail));
    if (item.value) select.append(el('span', 'menu-value', item.value));
    row.append(select);
    if (item.secondaryId) {
      const secondaryId = item.secondaryId;
      const test = iconButton('gauge', item.secondaryLabel || `测试 ${item.label} 延迟`, () => onAction(secondaryId), 'node-test'); test.disabled = Boolean(item.disabled); row.append(test);
    }
    list.append(row);
  }
  list.addEventListener('keydown', event => {
    if (event.isComposing || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    const controls = [...list.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    const index = controls.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? controls.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + controls.length) % controls.length;
    event.preventDefault(); controls[next]?.focus();
  });
  fragment.append(list); return fragment;
}

export async function mountSubmenu() {
  document.body.classList.add('submenu-window');
  const applyTheme = () => { const theme = localStorage.getItem('clashbar-appearance'); document.documentElement.dataset.appearance = theme === '深色' ? 'dark' : theme === '浅色' ? 'light' : 'system'; };
  applyTheme(); window.addEventListener('storage', applyTheme);
  const root = document.querySelector<HTMLElement>('#app')!;
  let currentId: string | null = null;
  const update = (model: MenuModel | null) => {
    applyTheme();
    currentId = model?.id ?? null;
    if (!model) { root.replaceChildren(); return; }
    const previous = document.activeElement?.getAttribute('aria-label');
    root.replaceChildren(renderMenu(model, actionId => { void invoke('attached_menu_action', { menuId: model.id, actionId }); }));
    const focused = previous ? [...root.querySelectorAll<HTMLElement>('[aria-label]')].find(node => node.getAttribute('aria-label') === previous) : null;
    (focused ?? root.querySelector<HTMLElement>('[aria-checked="true"], button:not(:disabled)'))?.focus();
  };
  await listen<MenuModel | null>('menu-data', event => update(event.payload));
  await listen('menu-focus', () => { (root.querySelector<HTMLElement>('[aria-checked="true"]') ?? root.querySelector<HTMLElement>('button:not(:disabled)'))?.focus(); });
  update(await invoke<MenuModel | null>('get_attached_menu'));
  document.body.addEventListener('pointerenter', () => { if (currentId) void invoke('attached_menu_hover', { menuId: currentId, hovered: true }); });
  document.body.addEventListener('pointerleave', () => { if (currentId) void invoke('attached_menu_hover', { menuId: currentId, hovered: false }); });
  document.addEventListener('keydown', event => { if (event.key === 'Escape' && !event.isComposing) { event.preventDefault(); void invoke('hide_attached_menu'); } });
}
