// A short-lived navigation hint only: never store proxy addresses, credentials,
// scan candidates or check authority in WebView storage.
const key = 'hiroute.subscription-proxy-return';
const lifetime = 5 * 60 * 1000;

export function requestSubscriptionProxyReturn() {
  window.localStorage.setItem(key, String(Date.now()));
}

export function clearSubscriptionProxyReturn() {
  try { window.localStorage.removeItem(key); } catch { /* Navigation storage is optional at startup. */ }
}

export function takeSubscriptionProxyReturn(): boolean {
  try {
    const value = window.localStorage.getItem(key);
    window.localStorage.removeItem(key);
    if (!value || !/^\d+$/.test(value)) return false;
    const age = Date.now() - Number(value);
    return age >= 0 && age < lifetime;
  } catch { return false; }
}
