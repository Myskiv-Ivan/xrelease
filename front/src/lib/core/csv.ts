/** Escape a single CSV field (RFC 4180). */
export function csvEscape(value: string | number | boolean | null | undefined): string {
	if (value == null) return '';
	const text = String(value);
	if (/[",\r\n]/.test(text)) {
		return `"${text.replace(/"/g, '""')}"`;
	}
	return text;
}

export function toCsv(
	headers: string[],
	rows: Array<Array<string | number | boolean | null | undefined>>
): string {
	const lines = [
		headers.map(csvEscape).join(','),
		...rows.map((row) => row.map(csvEscape).join(','))
	];
	return `${lines.join('\r\n')}\r\n`;
}

/** Trigger a browser download for a CSV string. */
export function downloadCsv(filename: string, csv: string): void {
	const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
	const url = URL.createObjectURL(blob);
	const anchor = document.createElement('a');
	anchor.href = url;
	anchor.download = filename;
	anchor.rel = 'noopener';
	document.body.appendChild(anchor);
	anchor.click();
	anchor.remove();
	URL.revokeObjectURL(url);
}

/** Convenience: dated filename + download for a filtered table export. */
export function exportRowsAsCsv(
	filenamePrefix: string,
	headers: string[],
	rows: Array<Array<string | number | boolean | null | undefined>>
): void {
	const stamp = new Date().toISOString().slice(0, 10);
	downloadCsv(`${filenamePrefix}-${stamp}.csv`, toCsv(headers, rows));
}
