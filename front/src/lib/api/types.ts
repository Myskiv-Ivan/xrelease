/**
 * API types — thin aliases over the generated OpenAPI schema.
 *
 * `schema.d.ts` is generated from `./././api/openapi.json` (the single
 * source of truth shared with the Rust backend) by `npm run gen:api`;
 * `npm run check:api` (part of `npm run check`) fails when the generated
 * file drifts from the spec. Do not hand-edit `schema.d.ts` — regenerate it.
 */

import type { components } from './schema';

type Schemas = components['schemas'];

export type HealthResponse = Schemas['HealthResponse'];
export type ReadyResponse = Schemas['ReadyResponse'];
export type SourceMetricsView = Schemas['SourceMetricsView'];
export type SourceSummary = Schemas['SourceSummary'];
export type SourceDetail = Schemas['SourceDetail'];
export type SeenReleaseView = Schemas['SeenReleaseView'];
export type AdvisoryView = Schemas['AdvisoryView'];
export type MetricsSnapshot = Schemas['MetricsSnapshot'];
export type StatusResponse = Schemas['StatusResponse'];
export type ConfigApplyStatus = Schemas['ConfigApplyStatus'];
export type AdvisoryStatus = Schemas['AdvisoryStatus'];
export type ConfigView = Schemas['ConfigView'];
export type ConfigSchema = Schemas['ConfigSchema'];
export type ConfigRevisionSummary = Schemas['ConfigRevisionSummary'];
export type ConfigRevisionsResponse = Schemas['ConfigRevisionsResponse'];
export type ConfigValidateResponse = Schemas['ConfigValidateResponse'];
export type ConfigApplyResponse = Schemas['ConfigApplyResponse'];
export type OutboxEntry = Schemas['OutboxEntry'];
export type OutboxStatus = OutboxEntry['status'];
export type OutboxListResponse = Schemas['OutboxListResponse'];
export type OutboxRequeueResponse = Schemas['OutboxRequeueResponse'];
export type TeamView = Schemas['TeamView'];
export type TeamListResponse = Schemas['TeamListResponse'];
export type CheckResponse = Schemas['CheckResponse'];
export type NotifierView = Schemas['NotifierView'];
export type NotifierListResponse = Schemas['NotifierListResponse'];
export type NotifierTestRequest = Schemas['NotifierTestRequest'];
export type NotifierTestResult = Schemas['NotifierTestResult'];
export type NotifierTestResponse = Schemas['NotifierTestResponse'];

/** Local UI login / session types from OpenAPI. */
export type AppRoleName = Schemas['AppRole'];
	export type AuthLoginRequest = Schemas['AuthLoginRequest'];
	export type AuthUserView = Schemas['AuthUserView'];
	export type AuthLoginResponse = Schemas['AuthLoginResponse'];
	export type AuthMethodsResponse = Schemas['AuthMethodsResponse'];
	export type AuthMeResponse = Schemas['AuthMeResponse'];
	export type AuthUserListResponse = Schemas['AuthUserListResponse'];
	export type AuthCreateUserRequest = Schemas['AuthCreateUserRequest'];

/** Organizations (multi-tenant). */
export type OrganizationView = Schemas['OrganizationView'];
export type OrganizationListResponse = Schemas['OrganizationListResponse'];
export type OrganizationConfigView = Schemas['OrganizationConfigView'];
export type OrganizationRevisionsResponse = Schemas['OrganizationRevisionsResponse'];

export type ApiError = Schemas['ErrorResponse'];
