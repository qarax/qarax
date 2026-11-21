import createClient, { type Middleware } from 'openapi-fetch';
import type { components, paths } from './schema';
import { session } from '#lib/session.svelte.ts';

type Schemas = components['schemas'];
export type Vm = Schemas['Vm'];
export type VmStatus = Schemas['VmStatus'];
export type Host = Schemas['Host'];
export type HostStatus = Schemas['HostStatus'];
export type HostCapacity = Schemas['HostResourceCapacity'];
export type Hypervisor = Schemas['Hypervisor'];
export type NetworkInterface = Schemas['NetworkInterface'];
export type Network = Schemas['Network'];
export type NewVm = Schemas['NewVm'];
export type StoragePool = Schemas['StoragePool'];
export type StoragePoolStatus = Schemas['StoragePoolStatus'];
export type StoragePoolType = Schemas['StoragePoolType'];
export type StorageObject = Schemas['StorageObject'];
export type StorageObjectType = Schemas['StorageObjectType'];
export type NetworkStatus = Schemas['NetworkStatus'];
export type TransferStatus = Schemas['TransferStatus'];

/** A request the control plane answered with an error status. */
export class ApiError extends Error {
	readonly status: number;

	constructor(message: string, status: number) {
		super(message);
		this.status = status;
	}
}

const authorize: Middleware = {
	onRequest({ request }) {
		const token = session.token;
		if (token) request.headers.set('Authorization', `Bearer ${token}`);
		return request;
	},
	onResponse({ response }) {
		if (response.status === 401) session.expire();
		return response;
	}
};

/** The API, on the origin that served the UI. */
export const api = createClient<paths>();
api.use(authorize);

type Result<T> = { data?: T; error?: unknown; response: Response };

/** The data of a successful response; an {@link ApiError} for any other. */
export async function call<T>(request: Promise<Result<T>>): Promise<T> {
	const { data, error, response } = await request;
	if (!response.ok) throw new ApiError(describe(error, response), response.status);
	return data as T;
}

/** The control plane's errors are `{ "message": ... }`, except the plain text of maintenance mode. */
function describe(error: unknown, response: Response): string {
	if (typeof error === 'object' && error !== null && 'message' in error)
		return String(error.message);
	if (typeof error === 'string' && error.trim()) return error.trim();
	return `The control plane answered ${response.status} ${response.statusText}.`;
}

type Page<T> = { items: T[]; next_cursor?: string | null };

/** Every item of a paged list, a page of up to 1000 at a time. */
async function listAll<T>(
	fetchPage: (cursor: string | undefined) => Promise<Page<T>>
): Promise<T[]> {
	const items: T[] = [];
	let cursor: string | undefined;
	do {
		const page = await fetchPage(cursor);
		items.push(...page.items);
		cursor = page.next_cursor ?? undefined;
	} while (cursor);
	return items;
}

export const allVms = () =>
	listAll((cursor) => call(api.GET('/v1/vms', { params: { query: { limit: 1000, cursor } } })));

export const allHosts = () =>
	listAll((cursor) => call(api.GET('/v1/hosts', { params: { query: { limit: 1000, cursor } } })));

export const allNetworks = () =>
	listAll((cursor) =>
		call(api.GET('/v1/networks', { params: { query: { limit: 1000, cursor } } }))
	);

export const allInstanceTypes = () =>
	listAll((cursor) =>
		call(api.GET('/v1/instance-types', { params: { query: { limit: 1000, cursor } } }))
	);

export const allBootSources = () =>
	listAll((cursor) =>
		call(api.GET('/v1/boot-sources', { params: { query: { limit: 1000, cursor } } }))
	);

export const allSecurityGroups = () =>
	listAll((cursor) =>
		call(api.GET('/v1/security-groups', { params: { query: { limit: 1000, cursor } } }))
	);

export const allStoragePools = () =>
	listAll((cursor) =>
		call(api.GET('/v1/storage-pools', { params: { query: { limit: 1000, cursor } } }))
	);

/** The storage objects of a type, or of every type, in one pool or in all of them. */
export const allStorageObjects = (filter: { objectType?: StorageObjectType; poolId?: string }) =>
	listAll((cursor) =>
		call(
			api.GET('/v1/storage-objects', {
				params: {
					query: { object_type: filter.objectType, pool_id: filter.poolId, limit: 1000, cursor }
				}
			})
		)
	);

export const allTransfers = (poolId: string) =>
	listAll((cursor) =>
		call(
			api.GET('/v1/storage-pools/{pool_id}/transfers', {
				params: { path: { pool_id: poolId }, query: { limit: 1000, cursor } }
			})
		)
	);

export const hostCapacity = (hostId: string) =>
	call(api.GET('/v1/hosts/{host_id}/resources', { params: { path: { host_id: hostId } } }));

/** Each host with its capacity. */
export const hostsWithCapacity = async () =>
	Promise.all(
		(await allHosts()).map(async (host) => ({ host, capacity: await hostCapacity(host.id) }))
	);
