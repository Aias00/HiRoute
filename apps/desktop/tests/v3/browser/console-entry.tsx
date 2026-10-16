import React from 'react';
import { createRoot } from 'react-dom/client';
import { ConsoleRoot } from '../../../src/host/ConsoleRoot';
import { consoleTransport } from '../../../src/host/console';
import { DesktopApp } from '../../../src/product/DesktopApp';
import { mockProductInvoke } from './product-fixtures';
import '../../../src/occami/styles.css';

const query = new URLSearchParams(location.search);
const state = query.get('case') ?? 'login';
localStorage.setItem('hiroute.language', query.get('language') ?? 'zh');
localStorage.setItem('hiroute.theme', query.get('theme') ?? 'light');
sessionStorage.clear();
if (!['login', 'launch'].includes(state)) sessionStorage.setItem('hiroute.console.session', 'b'.repeat(64));
if (state === 'launch') history.replaceState(null, '', '/launch?code=' + 'a'.repeat(64));
const control = {
  requests: [] as { path: string; headers: Record<string, string>; credentials?: RequestCredentials; url: string; body: unknown }[],
  expired: state === 'expired', unavailable: state === 'unavailable',
  errors: [] as string[],
  invoke: consoleTransport.call,
};
const networkFetch = window.fetch.bind(window);
window.fetch = async (input, options) => {
  const url = new URL(String(input), location.href);
  if (!url.pathname.startsWith('/api/console/v1/')) return networkFetch(input, options);
  const headers = new Headers(options?.headers);
  control.requests.push({ path: url.pathname, headers: Object.fromEntries(headers), credentials: options?.credentials, url: location.href, body: options?.body ? JSON.parse(String(options.body)) : null });
  if (control.unavailable) throw new TypeError('Synthetic disconnected service');
  const reply = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  if (/\/session\/(launch|token)$/.test(url.pathname)) return reply({ bearer: 'b'.repeat(64) });
  if (control.expired) return reply({ kind: 'native', code: 'CONSOLE_AUTH_REQUIRED' }, 401);
  if (url.pathname.endsWith('/session')) return reply({
    session: { id: 'session/component', credential_kind: 'access_token', token_id: 'token/component', expires: 9999999999 },
    transport: 'network_plaintext', assets_available: state !== 'assets', capabilities: { platform: 'linux', diagnostic_directory: '/example/diagnostics', recovery_directory: '/example/service' },
  });
  const command = decodeURIComponent(url.pathname.split('/').at(-1)!);
  const args = options?.body ? JSON.parse(String(options.body)) : {};
  if (command === 'observe_operation' || command === 'web_confirmation_snapshot') return reply(null);
  if (command === 'worker_task_list') return reply({ status: 'succeeded', data: { schema: 'hiroute.worker-task-list/v1', tasks: [], next_cursor: null }, next_actions: [], error: null });
  try { return reply(await mockProductInvoke(command, args, 'ready') ?? null); }
  catch (error) { control.errors.push(String(error)); return reply({ kind: 'native', code: 'FIXTURE_COMMAND_UNHANDLED' }, 500); }
};
Object.assign(window, { consoleComponent: control });
createRoot(document.getElementById('root')!).render(<ConsoleRoot><DesktopApp /></ConsoleRoot>);
