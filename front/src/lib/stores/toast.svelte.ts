export type ToastTone = 'default' | 'success' | 'error';

export interface ToastMessage {
	id: number;
	title: string;
	description?: string;
	tone: ToastTone;
}

let toasts = $state<ToastMessage[]>([]);
let nextId = 0;

function removeToast(id: number) {
	toasts = toasts.filter((toast) => toast.id !== id);
}

export function pushToast(input: Omit<ToastMessage, 'id'>) {
	const id = ++nextId;
	toasts = [...toasts, { ...input, id }];
	setTimeout(() => removeToast(id), 4_500);
}

export function getToastState() {
	return {
		get items() {
			return toasts;
		},
		dismiss: removeToast
	};
}
