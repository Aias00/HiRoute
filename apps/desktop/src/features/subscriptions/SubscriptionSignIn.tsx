import { SecretTransportWarning } from '../../host/ConsoleUi';
import { useEffect, useRef, useState } from 'react';
import { invoke } from '../../host/index.ts';
import { BrandIcon, Disclosure, UiIcon } from '../../ui';
import { safeDiagnosticCode } from '../../error-code';
import type { CandidateRef, SubscriptionLoginProvider, SubscriptionLoginRequest, SubscriptionLoginResult, SubscriptionLoginSession } from './types';

type Props = {
  language: 'zh' | 'en';
  trustedAuthority: boolean;
  onReuseNative(): void;
  onConnect(candidate: CandidateRef): Promise<void>;
};

const providers: SubscriptionLoginProvider[] = ['codex', 'claude'];

async function manage(request: SubscriptionLoginRequest): Promise<SubscriptionLoginResult> {
  return invoke<SubscriptionLoginResult>('manage_subscription_login', { request });
}

export function SubscriptionSignIn({ language, trustedAuthority, onReuseNative, onConnect }: Props) {
  const [sessions, setSessions] = useState<SubscriptionLoginSession[]>([]);
  const [busy, setBusy] = useState<string>('loading');
  const [error, setError] = useState('');
  const [callback, setCallback] = useState('');
  const [callbackLogin, setCallbackLogin] = useState<string | null>(null);
  const [forgetLogin, setForgetLogin] = useState<string | null>(null);
  const alive = useRef(true);
  const current = useRef<SubscriptionLoginSession[]>([]);
  const pendingOwned = useRef(new Set<string>());
  const inFlight = useRef(false);
  const generation = useRef(0);
  const text = (zh: string, en: string) => language === 'zh' ? zh : en;

  function merge(incoming: SubscriptionLoginSession[]) {
    const next = [...current.current];
    for (const session of incoming) {
      const index = next.findIndex(item => item.login_ref === session.login_ref);
      // Start's browser URL is kept only in this component's memory while pending.
      const retainedUrl = session.status === 'pending' ? next[index]?.authorization_url : undefined;
      const value = { ...session, authorization_url: session.authorization_url ?? retainedUrl };
      if (index < 0) next.push(value); else next[index] = value;
      if (session.status !== 'pending') pendingOwned.current.delete(session.login_ref);
    }
    current.current = next;
    if (alive.current) setSessions(next);
  }

  useEffect(() => {
    alive.current = true;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function observe() {
      if (stopped) return;
      if (!inFlight.current) {
        const observedGeneration = generation.current;
        try {
          for (const session of current.current.filter(item => item.status === 'pending')) {
            const result = await manage({ action: 'status', login_ref: session.login_ref });
            if (!stopped && generation.current === observedGeneration) merge(result.sessions);
          }
        } catch (cause) {
          if (!stopped && generation.current === observedGeneration) setError(safeDiagnosticCode(cause, 'SUBSCRIPTION_LOGIN_UNAVAILABLE'));
        }
      }
      if (!stopped) timer = setTimeout(() => void observe(), 2000);
    }
    void Promise.all(providers.map(provider => manage({ action: 'list', provider })))
      .then(results => { if (!stopped) merge(results.flatMap(result => result.sessions)); })
      .catch(cause => { if (!stopped) setError(safeDiagnosticCode(cause, 'SUBSCRIPTION_LOGIN_UNAVAILABLE')); })
      .finally(() => {
        if (!stopped) { setBusy(''); void observe(); }
      });
    return () => {
      stopped = true;
      alive.current = false;
      clearTimeout(timer);
      for (const login_ref of pendingOwned.current) void manage({ action: 'cancel', login_ref }).catch(() => {});
      pendingOwned.current.clear();
    };
  }, []);

  useEffect(() => {
    if (callbackLogin && sessions.some(session => session.login_ref === callbackLogin && session.status !== 'pending')) {
      setCallback('');
      setCallbackLogin(null);
    }
  }, [sessions, callbackLogin]);

  async function act(key: string, action: () => Promise<void>) {
    if (inFlight.current || !trustedAuthority) return;
    inFlight.current = true;
    generation.current += 1;
    setBusy(key);
    setError('');
    try { await action(); }
    catch (cause) { if (alive.current) setError(safeDiagnosticCode(cause, 'SUBSCRIPTION_LOGIN_UNAVAILABLE')); }
    finally { inFlight.current = false; if (alive.current) setBusy(''); }
  }

  async function start(provider: SubscriptionLoginProvider) {
    await act(provider, async () => {
      const result = await manage({ action: 'start', provider });
      if (!alive.current) {
        for (const session of result.sessions.filter(item => item.status === 'pending')) {
          await manage({ action: 'cancel', login_ref: session.login_ref });
        }
        return;
      }
      for (const session of result.sessions) {
        if (session.status === 'pending') pendingOwned.current.add(session.login_ref);
      }
      merge(result.sessions);
      const session = result.sessions.find(session => session.status === 'pending');
      if (session?.authorization_url && session.reason_code !== 'SUBSCRIPTION_LOGIN_MANUAL_CALLBACK') {
        await invoke('open_external_url', { url: session.authorization_url });
      }
    });
  }

  async function submitCallback(session: SubscriptionLoginSession) {
    const value = callback;
    setCallback('');
    await act(session.login_ref, async () => {
      const result = await invoke<SubscriptionLoginResult>('submit_subscription_login_callback', {
        loginRef: session.login_ref, callback: value,
      });
      if (alive.current) { merge(result.sessions); setCallbackLogin(null); }
    });
  }

  return <section className="oc-scan-list" aria-label={text('连接订阅', 'Connect subscription')}>
    <p className="v3-select-intro" data-autofocus tabIndex={-1}>{text('独立登录后，HiRoute 可自动刷新登录凭据，无需保持原生客户端运行。登录完成后，检查并选择模型，保存后才会接入。', 'After an independent sign-in, HiRoute can automatically refresh sign-in credentials without keeping the native client running. After signing in, check and choose models, then save to connect them.')}</p>
    {providers.map(provider => {
      const name = provider === 'codex' ? 'Codex' : 'Claude Code';
      const accounts = sessions.filter(session => session.provider === provider && !['cancelled', 'forgotten'].includes(session.status));
      const pending = accounts.some(session => session.status === 'pending');
      return <div className="oc-scan-detail" key={provider}>
        <div className="oc-status-row"><BrandIcon kind={provider === 'codex' ? 'codex' : 'claude-code'} label={name} /><div className="row-main"><strong>{name}</strong><p>{text('独立登录 · 推荐', 'Independent sign-in · Recommended')}</p></div><button type="button" className="btn btn-primary" disabled={!trustedAuthority || Boolean(busy) || pending} onClick={() => void start(provider)}>{pending ? text('等待登录', 'Waiting for sign-in') : text('登录', 'Sign in')}</button></div>
        {accounts.map(session => <div key={session.login_ref} className="oc-scan-detail">
          <p role="status">{session.status === 'authorized' ? text('已登录，可检查并选择模型', 'Signed in. Check and choose models.') : session.status === 'pending' ? session.reason_code === 'SUBSCRIPTION_LOGIN_MANUAL_CALLBACK' ? text('无法自动接收浏览器授权。请先结束本机其他登录，再打开登录页面；授权后复制完整返回地址，使用「手动完成授权」。', 'Automatic callback reception is unavailable. Finish any other local sign-in before opening the sign-in page, then copy the full return URL and use “Complete authorization manually”.') : text('请在浏览器中完成授权，HiRoute 会自动接收结果。若未自动完成，可手动粘贴完整返回地址。', 'Authorize in your browser; HiRoute will receive the result automatically. If it does not finish, you can paste the full return URL manually.') : session.status === 'expired' ? text('登录已过期，请重新登录', 'Sign-in expired. Start again.') : text('登录未完成，请重试', 'Sign-in did not complete. Try again.')}</p>
          <div className="task-actions">
            {session.status === 'pending' && <>
              {session.authorization_url && <button type="button" className="btn" disabled={!trustedAuthority || Boolean(busy)} onClick={() => void act(session.login_ref, () => invoke('open_external_url', { url: session.authorization_url }))}>{text('打开登录页面', 'Open sign-in page')}</button>}
              <button type="button" className="btn" disabled={!trustedAuthority || Boolean(busy)} onClick={() => void act(session.login_ref, async () => merge((await manage({ action: 'cancel', login_ref: session.login_ref })).sessions))}>{text('取消登录', 'Cancel sign-in')}</button>
              <button type="button" className="btn" disabled={!trustedAuthority || Boolean(busy)} onClick={() => { setCallback(''); setCallbackLogin(session.login_ref); }}>{text('手动完成授权', 'Complete authorization manually')}</button>
            </>}
            {session.status === 'authorized' && session.candidate && <button type="button" className="btn btn-primary" disabled={!trustedAuthority || Boolean(busy)} onClick={() => void act(session.login_ref, () => onConnect(session.candidate!))}>{text('检查并选择模型', 'Check and choose models')}</button>}
            {session.status !== 'pending' && <button type="button" className="btn" disabled={!trustedAuthority || Boolean(busy)} onClick={() => setForgetLogin(session.login_ref)}>{text('移除登录', 'Remove sign-in')}</button>}
          </div>
          {callbackLogin === session.login_ref && session.status === 'pending' && <form autoComplete="off" onSubmit={event => { event.preventDefault(); void submitCallback(session); }}><label className="field"><span>{text('粘贴浏览器地址栏中的完整回调地址或授权码', 'Paste the full callback URL from your browser address bar, or the authorization code')}</span><SecretTransportWarning language={language} /><input type="password" autoComplete="off" spellCheck={false} value={callback} onChange={event => setCallback(event.currentTarget.value)} /></label><p className="oc-meta">{text('返回页面无法打开时，也可复制地址栏。仅用于此次登录，请勿分享。', 'You can copy the address bar even if the return page cannot load. Used only for this sign-in; do not share it.')}</p><button className="btn" type="submit" disabled={!trustedAuthority || Boolean(busy) || !callback.trim()}>{text('提交授权', 'Submit authorization')}</button></form>}
          {forgetLogin === session.login_ref && <div className="callout warn"><p>{text('移除此登录会停止相关订阅连接。保留模型和路由配置，之后可重新登录。', 'Removing this sign-in stops its subscription connection. Model and routing settings remain available for a later sign-in.')}</p><button className="btn" type="button" disabled={!trustedAuthority || Boolean(busy)} onClick={() => void act(session.login_ref, async () => { merge((await manage({ action: 'forget', login_ref: session.login_ref })).sessions); if (alive.current) setForgetLogin(null); })}>{text('确认移除', 'Remove sign-in')}</button><button className="btn" type="button" disabled={!trustedAuthority || Boolean(busy)} onClick={() => setForgetLogin(null)}>{text('保留', 'Keep')}</button></div>}
        </div>)}
      </div>;
    })}
    <Disclosure label={text('复用本机登录', 'Reuse a local sign-in')} language={language}><p className="oc-meta">{text('本机 Codex / Claude Code 刷新登录凭据后，HiRoute 会定时同步。凭据刷新依赖原生客户端；未更新时，连接可能因登录过期而暂停。', 'HiRoute syncs sign-in credentials periodically after the local Codex / Claude Code client refreshes them. Credential refresh depends on the native client; the connection may pause if the sign-in expires.')}</p><p className="oc-meta">{text('Codex 仅支持复用文件中的登录；钥匙串等存储请使用独立登录。Claude Code 支持现有文件及 macOS 钥匙串登录。', 'Codex reuse supports file-based sign-ins; use independent sign-in for Keychain or other stores. Claude Code supports its existing file and macOS Keychain sign-ins.')}</p><button className="btn" type="button" disabled={!trustedAuthority || Boolean(busy)} onClick={onReuseNative}>{text('扫描本机登录', 'Find local sign-ins')}</button></Disclosure>
    {error && <div className="callout bad" role="alert" data-error-code={error}><UiIcon name="warning" /><span>{text('操作未完成，请重试。已接入的模型配置会保留。', 'The operation did not complete. Try again; existing model settings are retained.')}</span></div>}
  </section>;
}
