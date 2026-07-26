let isOnline = $state(true);

export function initNetworkMonitor() {
	if (typeof window === 'undefined') return;

	isOnline = navigator.onLine;

	const handleOnline = () => {
		isOnline = true;
	};
	const handleOffline = () => {
		isOnline = false;
	};

	window.addEventListener('online', handleOnline);
	window.addEventListener('offline', handleOffline);

	return () => {
		window.removeEventListener('online', handleOnline);
		window.removeEventListener('offline', handleOffline);
	};
}

export function getNetworkState() {
	return {
		get isOnline() {
			return isOnline;
		}
	};
}
