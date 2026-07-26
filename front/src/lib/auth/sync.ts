let authProfileSync: (() => void) | null = null;

export function registerAuthProfileSync(sync: () => void): void {
	authProfileSync = sync;
}

export function notifyAuthProfileSync(): void {
	authProfileSync?.();
}
