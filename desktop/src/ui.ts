import { t } from './i18n';
export function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = '', text?: string): HTMLElementTagNameMap[K] {
    const node = document.createElement(tag);
    node.className = className;
    if (text !== undefined)
        node.textContent = text;
    return node;
}
export function button(label: string, action: () => void | Promise<unknown>, className = '') {
    const node = el('button', `button ${className}`, label);
    node.type = 'button';
    node.addEventListener('click', () => { void action(); });
    return node;
}
const paths: Record<string, string> = {
    pin: 'M8 3h8l-2 6 3 3v2H7v-2l3-3-2-6M12 14v7',
    restart: 'M20 7v5h-5M20 12a8 8 0 1 0-2 6M20 7l-4-4',
    stop: 'M8 8h8v8H8z', play: 'M9 7l8 5-8 5z', power: 'M12 2v10M6 5a9 9 0 1 0 12 0',
    shield: 'M12 2l8 3v6c0 5-8 10-8 10S4 16 4 11V5l8-3M12 2v19',
    globe: 'M2 12h20M12 2c6 5 6 15 0 20-6-5-6-15 0-20M4 6h16M4 18h16',
    bolt: 'M13 2L5 14h6l-1 8 9-13h-7z', document: 'M6 2h8l5 5v15H6zM14 2v6h5M9 12h7M9 16h7',
    terminal: 'M3 4h18v16H3zM6 8l4 4-4 4M13 16h5', drive: 'M5 5h14l3 12H2zM2 17v4h20v-4M6 19h2',
    chip: 'M6 6h12v12H6zM9 9h6v6H9zM8 2v4M12 2v4M16 2v4M8 18v4M12 18v4M16 18v4M2 8h4M2 12h4M2 16h4M18 8h4M18 12h4M18 16h4',
    link: 'M10 14l4-4M8 16l-2 2a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0M16 8l2-2a4 4 0 0 1 6 6l-5 5a4 4 0 0 1-6 0',
    chevron: 'M9 5l7 7-7 7', down: 'M5 9l7 7 7-7', copy: 'M8 8h12v13H8zM4 16H2V2h12v3',
    x: 'M6 6l12 12M18 6L6 18', search: 'M20 20l-5-5M17 10a7 7 0 1 1-14 0 7 7 0 0 1 14 0',
    refresh: 'M20 7v5h-5M4 17v-5h5M20 12a8 8 0 0 0-14-6M4 12a8 8 0 0 0 14 6',
    gauge: 'M4 19a10 10 0 1 1 16 0M12 14l4-8M6 9l1 1M12 5v2M18 9l-1 1M4 15h2M18 15h2',
    eye: 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7-10-7-10-7M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
    list: 'M8 5h14M8 12h14M8 19h14M2 4h2v3M2 10h2l-2 4h2M2 17h2v4H2',
    chart: 'M3 12h4v9H3zM10 7h4v14h-4zM17 2h4v19h-4z',
    up: 'M12 19V5M6 11l6-6 6 6', arrowDown: 'M12 5v14M6 13l6 6 6-6',
    trash: 'M3 6h18M6 6l1 16h10l1-16M9 6V2h6v4M10 10v7M14 10v7',
    check: 'M4 12l5 5L20 6', pause: 'M7 4v16M17 4v16',
    settings: 'M4 5h16M4 12h16M4 19h16M8 2v6M16 9v6M9 16v6',
    network: 'M12 4v8M4 12h16M4 12v6M20 12v6M9 1h6v6H9zM1 18h6v5H1zM17 18h6v5h-6z',
    clock: 'M12 5v7l4 2', info: 'M12 10v7M12 6v1', warning: 'M12 2L1 22h22zM12 9v6M12 18v1',
    folder: 'M2 5h8l2 3h10v13H2z', sun: 'M12 1v3M12 20v3M1 12h3M20 12h3M4 4l2 2M18 18l2 2M4 20l2-2M18 6l2-2',
};
export function icon(name: string, className = '') {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('class', `icon ${className}`);
    svg.setAttribute('aria-hidden', 'true');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '1.7');
    svg.setAttribute('stroke-linecap', 'round');
    svg.setAttribute('stroke-linejoin', 'round');
    if (['globe', 'clock', 'info', 'stop', 'play', 'sun'].includes(name)) {
        const circle = document.createElementNS(svg.namespaceURI, 'circle');
        circle.setAttribute('cx', '12');
        circle.setAttribute('cy', '12');
        circle.setAttribute('r', name === 'sun' ? '5' : '10');
        svg.append(circle);
    }
    const path = document.createElementNS(svg.namespaceURI, 'path');
    path.setAttribute('d', paths[name] ?? paths.settings);
    svg.append(path);
    return svg;
}
export function iconButton(name: string, label: string, action: () => void | Promise<unknown>, className = '') {
    const control = button('', action, `icon-button ${className}`);
    control.append(icon(name));
    control.setAttribute('aria-label', label);
    control.title = label;
    return control;
}
export function field(label: string, id: string, type = 'text') {
    const wrapper = el('div', 'field');
    const caption = el('label', '', label);
    caption.htmlFor = id;
    const input = el('input');
    input.id = id;
    input.type = type;
    const error = el('p', 'field-message');
    error.id = `${id}-message`;
    input.setAttribute('aria-describedby', error.id);
    wrapper.append(caption, input, error);
    return { wrapper, input, error };
}
export function setFieldError(input: HTMLInputElement, error: HTMLElement, message: string | null) {
    input.setAttribute('aria-invalid', String(Boolean(message)));
    error.textContent = message ?? '';
    error.classList.toggle('error-text', Boolean(message));
}
export function searchField(label: string, id: string, onSearch: (query: string) => void) {
    const wrap = el('div', 'search');
    const input = el('input');
    input.id = id;
    input.type = 'search';
    input.placeholder = label;
    input.setAttribute('aria-label', label);
    const clear = button(t("清除"), () => {
        input.value = '';
        clear.hidden = true;
        onSearch('');
        input.focus();
    }, 'ghost search-clear');
    clear.setAttribute('aria-label', t("清除{0}", label));
    clear.hidden = true;
    let composing = false;
    const update = () => { clear.hidden = !input.value; if (!composing)
        onSearch(input.value); };
    input.addEventListener('compositionstart', () => { composing = true; });
    input.addEventListener('compositionend', () => { composing = false; update(); });
    input.addEventListener('input', update);
    wrap.append(input, clear);
    return { wrap, input };
}
export function replacePreservingFocus(container: HTMLElement, ...nodes: Node[]) {
    const focus = document.activeElement;
    const id = focus instanceof HTMLElement && container.contains(focus) ? focus.id : '';
    const scroll = container.scrollTop;
    container.replaceChildren(...nodes);
    container.scrollTop = scroll;
    if (id)
        (document.getElementById(id) ?? document.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]'))?.focus({ preventScroll: true });
}
export function emptyState(title: string, description: string) {
    const box = el('div', 'empty-state');
    box.append(el('p', 'empty-title', title), el('p', 'muted', description));
    return box;
}
export function presentDialog(dialog: HTMLDialogElement, initialFocus: HTMLElement, restoreFocus: HTMLElement, pending: () => boolean) {
    dialog.addEventListener('cancel', event => { if (pending())
        event.preventDefault(); });
    dialog.addEventListener('keydown', event => {
        if (event.key !== 'Tab')
            return;
        const controls = [...dialog.querySelectorAll<HTMLElement>('input:not(:disabled),button:not(:disabled),textarea:not(:disabled),select:not(:disabled),a[href]')].filter(control => control.getClientRects().length > 0);
        if (!controls.length) {
            event.preventDefault();
            return;
        }
        if (event.shiftKey && document.activeElement === controls[0]) {
            event.preventDefault();
            controls.at(-1)?.focus();
        }
        else if (!event.shiftKey && document.activeElement === controls.at(-1)) {
            event.preventDefault();
            controls[0]?.focus();
        }
    });
    dialog.addEventListener('close', () => { dialog.remove(); if (restoreFocus.isConnected)
        restoreFocus.focus(); }, { once: true });
    document.body.append(dialog);
    dialog.showModal();
    initialFocus.focus();
}
