export interface Status {
  running: boolean;
  corePath: string | null;
  configName: string | null;
  mixedPort: number;
  controllerPort: number;
  systemProxy: boolean;
  version: string | null;
  lastError: string | null;
}

export interface Proxy {
  name?: string;
  type: string;
  now?: string;
  all?: string[];
  history?: { delay: number; time?: string }[];
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
  chains?: string[];
  upload?: number;
  download?: number;
}

export interface Provider {
  type?: string;
  vehicleType?: string;
  updatedAt?: string;
  proxies?: Proxy[];
}

export interface Snapshot {
  proxies: { proxies: Record<string, Proxy> };
  configs: { mode: string };
  rules: { rules: Rule[] };
  connections: { connections: Connection[]; uploadTotal: number; downloadTotal: number };
  providers: { providers: Record<string, Provider> };
}

export type Tab = 'proxies' | 'rules' | 'connections' | 'logs' | 'settings';
