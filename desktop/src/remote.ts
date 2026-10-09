import { t } from './i18n';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { MachineConnectivity, RemoteMachine, Status } from './types';
import { button, el, field, icon, iconButton, presentDialog, setFieldError } from './ui';
import { safeError } from './utils';
interface MachineActions {
    status: () => Status | null;
    busy: () => boolean;
    error: () => string;
    mutate: (label: string, operation: () => Promise<unknown>, reload?: boolean) => Promise<boolean>;
    select: (id: string | null) => Promise<boolean>;
    resize: () => void;
}
export function openMachineManager(actions: MachineActions, trigger: HTMLElement) {
    if (document.querySelector('.machine-manager[open]'))
        return;
    const dialog = el('dialog', 'dialog machine-manager');
    dialog.setAttribute('aria-labelledby', 'machine-title');
    const header = el('div', 'machine-heading');
    const title = el('h2', '', t("管理远程机器"));
    title.id = 'machine-title';
    const close = iconButton('x', t("关闭"), () => { if (!pending && !actions.busy()) {
        if (view === 'list')
            dialog.close();
        else if (view === 'edit')
            requestLeaveEditor();
        else if (view === 'discard')
            resumeEditor?.();
        else
            returnToList();
    } });
    header.append(icon('network'), title, el('span', 'flex-space'), close);
    const body = el('div', 'machine-body');
    const feedback = el('p', 'field-message error-text');
    feedback.setAttribute('aria-live', 'polite');
    const footer = el('div', 'machine-footer');
    dialog.append(header, body, feedback, footer);
    let pending = false, view: 'list' | 'edit' | 'delete' | 'discard' = 'list', generation = 0, probing = false;
    let editorDirty = () => false;
    let resumeEditor: (() => void) | null = null;
    const checks = new Map<string, MachineConnectivity>();
    const rows = new Map<string, {
        control: HTMLButtonElement;
        status: HTMLElement;
    }>();
    const setPending = (value: boolean) => {
        pending = value;
        close.disabled = value;
        for (const control of dialog.querySelectorAll<HTMLButtonElement | HTMLInputElement>('button,input'))
            control.disabled = value;
        dialog.setAttribute('aria-busy', String(value));
    };
    function setView(next: typeof view, label: string) {
        view = next;
        generation++;
        title.textContent = label;
        feedback.textContent = '';
        body.replaceChildren();
        footer.replaceChildren();
        close.replaceChildren(icon(next === 'list' ? 'x' : 'chevron'));
        close.classList.toggle('machine-back', next !== 'list');
        close.setAttribute('aria-label', next === 'list' ? t("关闭") : next === 'discard' ? t("返回编辑") : t("返回机器列表"));
    }
    function updateConnectivity(machine: RemoteMachine) {
        const row = rows.get(machine.id);
        if (!row)
            return;
        const check = checks.get(machine.id);
        row.control.disabled = pending || !check?.connected || actions.status()?.activeRemoteId === machine.id;
        row.status.textContent = check ? check.connected ? t("已连接 · {0}", check.version ?? '—') : t("连接失败 · {0}", check.error ? safeError(check.error) : t("请检查地址与密钥")) : t("检查中…");
        row.status.classList.toggle('green', Boolean(check?.connected));
    }
    async function probe() {
        if (probing || pending || actions.busy() || view !== 'list' || !dialog.open || document.hidden)
            return;
        probing = true;
        const ticket = generation;
        await Promise.all((actions.status()?.remoteMachines ?? []).map(async (machine) => {
            let result: MachineConnectivity;
            try {
                result = await invoke<MachineConnectivity>('check_remote_machine', { id: machine.id });
            }
            catch (error) {
                result = { id: machine.id, connected: false, version: null, error: safeError(error) };
            }
            if (ticket !== generation || !dialog.open || view !== 'list')
                return;
            checks.set(machine.id, result);
            updateConnectivity(machine);
        }));
        probing = false;
    }
    async function select(id: string | null) {
        if (pending || actions.busy())
            return;
        setPending(true);
        const success = await actions.select(id);
        setPending(false);
        if (success)
            dialog.close();
        else {
            feedback.textContent = actions.error();
            for (const machine of actions.status()?.remoteMachines ?? [])
                updateConnectivity(machine);
        }
    }
    function renderList() {
        editorDirty = () => false;
        resumeEditor = null;
        setView('list', t("管理远程机器"));
        rows.clear();
        const local = button('', () => select(null), 'machine-local');
        local.disabled = !actions.status()?.activeRemoteId;
        const info = el('span', 'machine-info');
        info.append(el('strong', '', t("本机")), el('small', 'muted', `127.0.0.1:${actions.status()?.controllerPort ?? 19090}`));
        local.append(icon('chip', 'blue'), info);
        if (!actions.status()?.activeRemoteId)
            local.append(icon('check', 'green'));
        body.append(local);
        for (const machine of actions.status()?.remoteMachines ?? []) {
            const row = el('div', 'machine-row');
            const control = button('', () => select(machine.id), 'machine-select');
            const info = el('span', 'machine-info');
            const address = el('small', 'muted', machine.address);
            address.title = machine.address;
            const status = el('small', 'machine-state');
            info.append(el('strong', '', machine.name), address, status);
            control.append(icon('network', 'blue'), info);
            row.append(control);
            if (actions.status()?.activeRemoteId === machine.id)
                row.append(icon('check', 'green'));
            row.append(iconButton('settings', t("编辑机器 {0}", machine.name), () => renderEditor(machine)), iconButton('trash', t("删除机器 {0}", machine.name), () => renderDelete(machine), 'danger-text'));
            rows.set(machine.id, { control, status });
            body.append(row);
            updateConnectivity(machine);
        }
        const add = button(t("添加机器"), () => renderEditor(), 'small-action primary');
        footer.append(add);
        void probe();
        return add;
    }
    function returnToList() { const focus = renderList(); focus.focus(); }
    function requestLeaveEditor() {
        if (!editorDirty()) {
            returnToList();
            return;
        }
        const content = [...body.childNodes], actions = [...footer.childNodes], previousTitle = title.textContent ?? t("编辑机器");
        const previousFocus = document.activeElement as HTMLElement;
        setView('discard', t("放弃未保存的更改？"));
        body.append(el('p', 'dialog-copy', t("机器信息尚未保存。可以继续编辑，或放弃这次修改。")));
        resumeEditor = () => { setView('edit', previousTitle); body.append(...content); footer.append(...actions); if (previousFocus.isConnected)
            previousFocus.focus(); };
        const keep = button(t("继续编辑"), () => resumeEditor?.(), 'small-action');
        footer.append(keep, button(t("放弃修改"), returnToList, 'small-action danger-text'));
        keep.focus();
    }
    function renderEditor(machine?: RemoteMachine) {
        setView('edit', machine ? t("编辑机器") : t("添加机器"));
        const form = el('form', 'machine-form');
        form.noValidate = true;
        form.id = 'machine-editor-form';
        const name = field(t("名称"), 'machine-name');
        name.input.maxLength = 180;
        name.input.value = machine?.name ?? '';
        const host = field(t("主机地址"), 'machine-host');
        host.input.value = machine?.host ?? '';
        host.input.placeholder = 'controller.example.com';
        host.input.autocomplete = 'off';
        host.input.spellcheck = false;
        const port = field(t("端口"), 'machine-port', 'number');
        port.input.value = String(machine?.port ?? 9090);
        port.input.min = '1';
        port.input.max = '65535';
        const address = el('div', 'machine-address');
        address.append(host.wrapper, port.wrapper);
        const secret = field(t("密钥"), 'machine-secret', 'password');
        secret.input.autocomplete = 'new-password';
        secret.input.spellcheck = false;
        secret.input.placeholder = machine?.hasSecret ? t("留空保留已保存的密钥") : t("可选");
        secret.input.maxLength = 4096;
        const reveal = button(t("显示密钥"), () => { const show = secret.input.type === 'password'; secret.input.type = show ? 'text' : 'password'; reveal.textContent = show ? t("隐藏密钥") : t("显示密钥"); reveal.setAttribute('aria-pressed', String(show)); }, 'small-action');
        reveal.setAttribute('aria-pressed', 'false');
        secret.wrapper.append(reveal);
        const clearSecret = el('input');
        clearSecret.type = 'checkbox';
        clearSecret.id = 'machine-clear-secret';
        if (machine?.hasSecret) {
            const label = el('label', 'machine-clear-secret', t("清除已保存的密钥"));
            label.htmlFor = clearSecret.id;
            label.prepend(clearSecret);
            secret.wrapper.append(label);
            clearSecret.addEventListener('change', () => { secret.input.disabled = clearSecret.checked; });
        }
        const https = el('input', 'switch');
        https.type = 'checkbox';
        https.id = 'machine-https';
        https.checked = machine?.useHttps ?? false;
        const httpsRow = el('label', 'setting-row', 'HTTPS');
        httpsRow.htmlFor = https.id;
        httpsRow.append(el('span', 'flex-space'), https);
        const preview = el('p', 'machine-preview muted');
        const updatePreview = () => { const value = host.input.value.trim() || 'controller.example.com'; preview.textContent = t("预览：{0}://{1}:{2}", https.checked ? 'https' : 'http', value.includes(':') && !value.startsWith('[') ? `[${value}]` : value, port.input.value || '9090'); };
        for (const control of [host.input, port.input, https])
            control.addEventListener('input', updatePreview);
        updatePreview();
        const save = button(t("保存"), () => { }, 'small-action primary');
        save.type = 'submit';
        save.setAttribute('form', form.id);
        footer.append(save);
        form.append(preview, name.wrapper, address, secret.wrapper, httpsRow);
        body.append(form);
        name.input.focus();
        editorDirty = () => name.input.value !== (machine?.name ?? '') || host.input.value !== (machine?.host ?? '') || port.input.value !== String(machine?.port ?? 9090) || https.checked !== (machine?.useHttps ?? false) || Boolean(secret.input.value) || clearSecret.checked;
        let composing = false;
        form.addEventListener('compositionstart', () => { composing = true; });
        form.addEventListener('compositionend', () => { composing = false; });
        form.addEventListener('submit', event => {
            event.preventDefault();
            if (pending || actions.busy() || composing)
                return;
            const validName = name.input.value.trim().length > 0;
            const validHost = host.input.value.trim().length > 0 && !/[\s/\\@?#;%"'`$]/.test(host.input.value.trim()) && (!host.input.value.includes(':') || host.input.value.split(':').length > 2);
            const validPort = /^\d+$/.test(port.input.value) && Number(port.input.value) >= 1 && Number(port.input.value) <= 65535;
            const validSecret = clearSecret.checked || /^[\x20-\x7e]*$/.test(secret.input.value);
            setFieldError(name.input, name.error, validName ? null : t("请输入机器名称。"));
            setFieldError(host.input, host.error, validHost ? null : t("请输入主机名或 IP，端口单独填写。"));
            setFieldError(port.input, port.error, validPort ? null : t("端口需为 1–65535 之间的整数。"));
            setFieldError(secret.input, secret.error, validSecret ? null : t("密钥只能包含可打印 ASCII 字符。"));
            if (!validName || !validHost || !validPort || !validSecret) {
                (!validName ? name.input : !validHost ? host.input : !validPort ? port.input : secret.input).focus();
                return;
            }
            const credential = clearSecret.checked ? '' : secret.input.value || (machine ? null : '');
            setPending(true);
            save.setAttribute('aria-busy', 'true');
            void actions.mutate(t("保存远程机器"), () => invoke<Status>('save_remote_machine', { input: { id: machine?.id ?? null, name: name.input.value.trim(), host: host.input.value.trim(), port: Number(port.input.value), useHttps: https.checked, secret: credential } })).then(success => {
                setPending(false);
                save.setAttribute('aria-busy', 'false');
                secret.input.disabled = clearSecret.checked;
                if (success) {
                    secret.input.value = '';
                    checks.clear();
                    returnToList();
                }
                else {
                    feedback.textContent = actions.error();
                    name.input.focus();
                }
            });
        });
    }
    function renderDelete(machine: RemoteMachine) {
        setView('delete', t("删除机器"));
        body.append(el('p', 'dialog-copy', t("删除「{0}」及已保存的连接信息？{1}不会停止远程内核。", machine.name, actions.status()?.activeRemoteId === machine.id ? t("当前目标将切换回本机。") : '')));
        const cancel = button(t("取消"), returnToList, 'small-action');
        const remove = button(t("删除机器"), async () => {
            if (pending || actions.busy())
                return;
            setPending(true);
            const success = await actions.mutate(t("删除远程机器"), () => invoke<Status>('delete_remote_machine', { id: machine.id }));
            setPending(false);
            if (success) {
                checks.delete(machine.id);
                returnToList();
            }
            else {
                feedback.textContent = actions.error();
                cancel.focus();
            }
        }, 'small-action danger-text');
        footer.append(cancel, remove);
        cancel.focus();
    }
    const initial = renderList();
    dialog.addEventListener('cancel', event => {
        if (pending || actions.busy())
            return;
        if (view === 'edit') {
            event.preventDefault();
            requestLeaveEditor();
        }
        else if (view === 'discard') {
            event.preventDefault();
            resumeEditor?.();
        }
    });
    presentDialog(dialog, initial, trigger, () => pending || actions.busy());
    if (isTauri())
        void invoke('resize_popup', { height: 540 }).catch(() => { });
    const timer = setInterval(() => { void probe(); }, 5000);
    void probe();
    dialog.addEventListener('close', () => { generation++; clearInterval(timer); actions.resize(); }, { once: true });
}
