/** Shared clock for live relative timestamps ("2m" → "3m" without a refetch). */
const TICK_MS = 30_000;

let current = $state(new Date());
let intervalId: ReturnType<typeof setInterval> | null = null;
let subscribers = 0;

export function startNowTicker(): void {
	subscribers += 1;
	if (intervalId) return;
	intervalId = setInterval(() => {
		current = new Date();
	}, TICK_MS);
}

export function stopNowTicker(): void {
	subscribers = Math.max(0, subscribers - 1);
	if (subscribers > 0 || !intervalId) return;
	clearInterval(intervalId);
	intervalId = null;
}

export function getNowStore() {
	return {
		get current() {
			return current;
		}
	};
}
