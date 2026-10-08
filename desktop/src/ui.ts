export function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = '', text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function button(label: string, action: () => void | Promise<unknown>, className = '') {
  const node = el('button', `button ${className}`, label);
  node.type = 'button';
  node.addEventListener('click', () => { void action(); });
  return node;
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
  const clear = button('清除', () => {
    input.value = '';
    clear.hidden = true;
    onSearch('');
    input.focus();
  }, 'ghost search-clear');
  clear.setAttribute('aria-label', `清除${label}`);
  clear.hidden = true;
  let composing = false;
  const update = () => { clear.hidden = !input.value; if (!composing) onSearch(input.value); };
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
  if (id) document.getElementById(id)?.focus({ preventScroll: true });
}

export function emptyState(title: string, description: string) {
  const box = el('div', 'empty-state');
  box.append(el('p', 'empty-title', title), el('p', 'muted', description));
  return box;
}

export function table(headers: string[], rows: Node[][], label: string) {
  const wrap = el('div', 'table-scroll');
  wrap.tabIndex = 0;
  wrap.setAttribute('role', 'region');
  wrap.setAttribute('aria-label', `${label}，可滚动`);
  const node = el('table');
  const caption = el('caption', 'sr-only', label);
  const head = el('thead');
  const headerRow = el('tr');
  for (const name of headers) { const th = el('th', '', name); th.scope = 'col'; headerRow.append(th); }
  head.append(headerRow);
  const body = el('tbody');
  for (const cells of rows) {
    const row = el('tr');
    for (const cell of cells) { const td = el('td'); td.append(cell); row.append(td); }
    body.append(row);
  }
  node.append(caption, head, body);
  wrap.append(node);
  return wrap;
}

export function confirmAction(title: string, description: string, actionLabel: string, action: () => Promise<boolean>) {
  const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const dialog = el('dialog', 'dialog');
  const heading = el('h2', '', title); heading.id = 'dialog-title';
  const copy = el('p', 'dialog-copy', description); copy.id = 'dialog-description';
  dialog.setAttribute('aria-labelledby', heading.id);
  dialog.setAttribute('aria-describedby', copy.id);
  const message = el('p', 'field-message error-text');
  message.setAttribute('role', 'alert');
  const actions = el('div', 'dialog-actions');
  const cancel = button('取消', () => dialog.close());
  const submit = button(actionLabel, async () => {
    if (submit.disabled) return;
    cancel.disabled = submit.disabled = true;
    submit.setAttribute('aria-busy', 'true');
    message.textContent = '';
    const succeeded = await action();
    if (succeeded) dialog.close();
    else message.textContent = '操作未完成，请检查窗口内的错误提示后重试。';
    cancel.disabled = submit.disabled = false;
    submit.setAttribute('aria-busy', 'false');
    if (!succeeded) submit.focus();
  }, 'danger');
  actions.append(cancel, submit);
  dialog.append(heading, copy, message, actions);
  dialog.addEventListener('keydown', event => {
    if (event.key !== 'Tab' || event.isComposing) return;
    const controls = [...dialog.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    const first = controls[0], last = controls.at(-1);
    if (!first || !last) { event.preventDefault(); return; }
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
  });
  dialog.addEventListener('cancel', event => { if (submit.disabled) event.preventDefault(); });
  dialog.addEventListener('close', () => {
    dialog.remove();
    if (trigger?.isConnected) trigger.focus();
    else document.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
  }, { once: true });
  document.body.append(dialog);
  dialog.showModal();
  cancel.focus();
}
