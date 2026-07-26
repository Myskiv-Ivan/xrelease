export interface KeyValueItem {
	label: string;
	value: string | number | boolean;
	tone?: 'default' | 'success' | 'warning' | 'danger';
}
