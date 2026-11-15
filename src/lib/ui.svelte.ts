// App-wide UI state shared between components: the IP address context menu
// and the "who owns this address" dialog.

import { api, type IpOwner } from "./api";

export interface MenuState {
  x: number;
  y: number;
  address: string;
}

export interface OwnerState {
  address: string;
  loading: boolean;
  result: IpOwner | null;
  error: string;
}

export const ui = $state({
  menu: null as MenuState | null,
  owner: null as OwnerState | null,
});

export function openAddressMenu(e: MouseEvent, address: string) {
  e.preventDefault();
  e.stopPropagation();
  ui.menu = { x: e.clientX, y: e.clientY, address };
}

export async function lookUpOwner(address: string) {
  ui.menu = null;
  ui.owner = { address, loading: true, result: null, error: "" };
  try {
    const result = await api.ipOwner(address);
    if (ui.owner?.address === address) ui.owner = { address, loading: false, result, error: "" };
  } catch (e) {
    if (ui.owner?.address === address) ui.owner = { address, loading: false, result: null, error: String(e) };
  }
}
