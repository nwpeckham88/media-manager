export async function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
	const headers = new Headers(init?.headers ?? {});
	const token = localStorage.getItem('mm-api-token');
	if (token && token.trim().length > 0) {
		headers.set('Authorization', `Bearer ${token.trim()}`);
	}

	return fetch(input, {
		...init,
		headers
	});
}
