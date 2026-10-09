export interface Status {
  running: boolean;
  corePath: string | null;
  configName: string | null;
  mixedPort: number;
  controllerPort: number;
  systemProxy: boolean;
  version: string | null;
  lastError: string | null;
  profiles?: { id: string; name: string }[];
  activeProfileId?: string | null;
  activeRemoteId?: string | null;
  remoteMachines?: RemoteMachine[];
  controllerAddress?: string;
  targetName?: string;
  localRunning?: boolean;
  localMixedPort?: number;
  localLanAddress?: string | null;
  logStreamError?: string | null;
  systemProxyTarget?: string | null;
  systemProxyRemote?: boolean;
  targetRevision?: number;
  autoStartCore?: boolean;
  launchAtLogin?: boolean | null;
  launchAtLoginError?: string | null;
  localProxyPorts?: ProxyPorts;
  systemProxyExceptions?: string[];
  tunEnabled?: boolean;
  tunStack?: string | null;
  ssidEnabled?: boolean;
  ssidRules?: { ssid: string; configFileName: string }[];
  ssidSnapshot?: { currentSsid: string | null; status: string; error: string | null };
  ssidError?: string | null;
  subscriptions?: SubscriptionSummary[];
  statusBarStyle?: 'iconOnly' | 'iconAndSpeed' | 'speedOnly';
  uiLanguage?: 'zh-CN' | 'en';
  providerRefresh?: { running: boolean; done: number; total: number; failed: number; error: string | null };
}

export interface ProxyPorts { port: number; 'socks-port': number; 'mixed-port': number; 'redir-port': number; 'tproxy-port': number }
export interface SubscriptionSummary {
  profileId: string;
  sourceHost: string;
  autoUpdateEnabled: boolean;
  autoUpdateIntervalHours: number;
  lastCheckedAt: number | null;
  lastUpdatedAt: number | null;
  nextUpdateAt: number | null;
  lastError: string | null;
}
export interface AppReleaseInfo { currentVersion: string; tagName: string; displayVersion: string; name: string | null; releaseUrl: string; updateAvailable: boolean }

export interface RemoteMachine {
  id: string;
  name: string;
  host: string;
  port: number;
  useHttps: boolean;
  hasSecret: boolean;
  address: string;
}

export interface MachineConnectivity {
  id: string;
  connected: boolean;
  version: string | null;
  error: string | null;
}

export interface Proxy {
  name?: string;
  type: string;
  now?: string;
  all?: string[];
  history?: { delay: number; time?: string }[];
  hidden?: boolean;
  icon?: string;
}

export interface Rule {
  type: string;
  payload: string;
  proxy: string;
}

export interface Connection {
  id: string;
  metadata: {
    host?: string;
    destinationIP?: string;
    destinationPort?: string;
    process?: string;
    processPath?: string;
    network?: string;
    sourceIP?: string;
  };
  rule?: string;
  rulePayload?: string;
  start?: string;
  chains?: string[];
  upload?: number;
  download?: number;
}

export interface Provider {
  type?: string;
  vehicleType?: string;
  updatedAt?: string;
  proxies?: Proxy[];
  subscriptionInfo?: { upload: number; download: number; total: number; expire: number };
}

export interface Snapshot {
  proxies: { proxies: Record<string, Proxy> };
  configs: { mode: string; 'log-level'?: string | null; 'allow-lan'?: boolean | null; ipv6?: boolean | null; 'tcp-concurrent'?: boolean | null; 'mixed-port'?: number; port?: number; 'socks-port'?: number; 'redir-port'?: number; 'tproxy-port'?: number; tun?: { enable?: boolean; stack?: string }; 'external-ui'?: string };
  rules: { rules: Rule[] };
  connections: { connections: Connection[]; uploadTotal: number; downloadTotal: number };
  providers: { providers: Record<string, Provider> };
  ruleProviders: { providers: Record<string, RuleProvider> };
  memory?: number | null;
  traffic?: { up: number; down: number; upTotal?: number | null; downTotal?: number | null } | null;
}

export interface RuleProvider {
  name?: string;
  ruleCount?: number;
  updatedAt?: string;
  behavior?: string;
  vehicleType?: string;
}

export type Tab = 'proxies' | 'rules' | 'connections' | 'logs' | 'settings';
