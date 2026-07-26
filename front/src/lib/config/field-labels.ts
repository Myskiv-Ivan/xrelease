/**
 * Shared human labels / hints for config form fields (sources + notifiers).
 * Keeps ConfigEditPanel and ConfigNotifiersPanel on one vocabulary.
 */

import { t } from '$lib/i18n';

/** Optional hint under source secret-related fields. */
export function sourceFieldHint(key: string): string | null {
	switch (key) {
		case 'token':
			return t('config.fieldHintSourceToken');
		case 'token_env':
			return t('config.fieldHintSourceTokenEnv');
		default:
			return null;
	}
}

/** Primary source identity fields shown in the sources editor. */
export function sourceFieldLabel(key: string): string {
	switch (key) {
		case 'repo':
			return t('config.fieldRepo');
		case 'project':
			return t('config.fieldProject');
		case 'image':
			return t('config.fieldImage');
		case 'name':
			return t('config.fieldName');
		case 'url':
			return t('config.fieldUrl');
		case 'host':
			return t('config.fieldHost');
		case 'registry':
			return t('config.fieldRegistry');
		case 'edition':
			return t('config.fieldEdition');
		case 'package_kind':
			return t('config.fieldPackageKind');
		case 'token':
			return t('config.fieldSourceToken');
		case 'token_env':
			return t('config.fieldSourceTokenEnv');
		default:
			return key.replace(/_/g, ' ');
	}
}

/** Notifier channel field labels (secrets get explicit names, not raw keys). */
export function notifierFieldLabel(key: string): string {
	switch (key) {
		case 'endpoint':
			return t('config.fieldAppriseEndpoint');
		case 'urls':
			return t('config.fieldAppriseUrls');
		case 'urls_env':
			return t('config.fieldAppriseUrlsEnv');
		case 'format':
			return t('config.fieldAppriseFormat');
		case 'secret':
			return t('config.fieldSecret');
		case 'secret_env':
			return t('config.fieldSecretEnv');
		case 'headers':
			return t('config.fieldHeaders');
		case 'headers_env':
			return t('config.fieldHeadersEnv');
		case 'access_token':
			return t('config.fieldAccessToken');
		case 'access_token_env':
			return t('config.fieldAccessTokenEnv');
		case 'api_key':
			return t('config.fieldNovuApiKey');
		case 'api_key_env':
			return t('config.fieldNovuApiKeyEnv');
		case 'workflow':
			return t('config.fieldNovuWorkflow');
		case 'topic_key':
			return t('config.fieldNovuTopicKey');
		case 'subscriber_id':
			return t('config.fieldNovuSubscriberId');
		case 'webhook_url':
			return t('config.fieldSlackWebhookUrl');
		case 'webhook_url_env':
			return t('config.fieldSlackWebhookUrlEnv');
		case 'bot_token':
			return t('config.fieldBotToken');
		case 'bot_token_env':
			return t('config.fieldBotTokenEnv');
		case 'channel':
			return t('config.fieldSlackChannel');
		case 'chat_id':
			return t('config.fieldTelegramChatId');
		case 'parse_mode':
			return t('config.fieldTelegramParseMode');
		case 'password_env':
			return t('config.fieldPasswordEnv');
		case 'url_env':
			return t('config.fieldUrlEnv');
		case 'password':
			return t('config.fieldPassword');
		case 'subject_template':
			return t('config.fieldSubjectTemplate');
		default:
			return key.replace(/_/g, ' ');
	}
}

/** Optional hint under notifier secret-related fields. */
export function notifierFieldHint(key: string): string | null {
	switch (key) {
		case 'endpoint':
			return t('config.fieldHintAppriseEndpoint');
		case 'urls':
			return t('config.fieldHintAppriseUrls');
		case 'urls_env':
			return t('config.fieldHintAppriseUrlsEnv');
		case 'secret':
			return t('config.fieldHintSecret');
		case 'secret_env':
			return t('config.fieldHintSecretEnv');
		case 'headers':
			return t('config.fieldHintHeaders');
		case 'headers_env':
			return t('config.fieldHintHeadersEnv');
		case 'access_token':
			return t('config.fieldHintAccessToken');
		case 'access_token_env':
			return t('config.fieldHintAccessTokenEnv');
		case 'api_key':
			return t('config.fieldHintNovuApiKey');
		case 'api_key_env':
			return t('config.fieldHintNovuApiKeyEnv');
		case 'workflow':
			return t('config.fieldHintNovuWorkflow');
		case 'topic_key':
			return t('config.fieldHintNovuTopicKey');
		case 'subscriber_id':
			return t('config.fieldHintNovuSubscriberId');
		case 'webhook_url':
			return t('config.fieldHintSlackWebhookUrl');
		case 'webhook_url_env':
			return t('config.fieldHintSlackWebhookUrlEnv');
		case 'bot_token':
			return t('config.fieldHintBotToken');
		case 'bot_token_env':
			return t('config.fieldHintBotTokenEnv');
		case 'channel':
			return t('config.fieldHintSlackChannel');
		case 'chat_id':
			return t('config.fieldHintTelegramChatId');
		case 'parse_mode':
			return t('config.fieldHintTelegramParseMode');
		case 'password_env':
			return t('config.fieldHintPasswordEnv');
		case 'url_env':
			return t('config.fieldHintUrlEnv');
		case 'password':
			return t('config.fieldHintPassword');
		case 'subject_template':
			return t('config.fieldHintSubjectTemplate');
		case 'template':
			return t('config.fieldHintTemplate');
		default:
			return null;
	}
}
