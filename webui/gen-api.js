// Writes src/lib/api/schema.d.ts, the API's types, from ../openapi.yaml.
//
// The spec repeats operationIds (utoipa takes them from handler names, and most
// resources have a `list`, `get`, `delete`), and openapi-typescript keys its
// `operations` by operationId, so one resource's types would stand for all of
// theirs. Without operationIds each path carries its own types, which is all
// openapi-fetch goes by.
import { readFile, writeFile } from 'node:fs/promises';
import openapiTS, { astToString } from 'openapi-typescript';

const spec = await readFile(new URL('../openapi.yaml', import.meta.url), 'utf8');
const ast = await openapiTS(spec.replace(/^\s+operationId: .*\n/gm, ''));
await writeFile(new URL('src/lib/api/schema.d.ts', import.meta.url), astToString(ast));
