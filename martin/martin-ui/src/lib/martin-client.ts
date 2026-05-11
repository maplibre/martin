import createClient from 'openapi-fetch';
import { getMartinBaseUrl } from './api';
import type { paths } from './types.gen';

export const martinClient = createClient<paths>({
  baseUrl: getMartinBaseUrl().replace(/\/$/, ''),
});
