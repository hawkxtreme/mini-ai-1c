import { strict as assert } from 'node:assert';
import { normalizeUri } from '../uriUtils';

assert.equal(normalizeUri(''), '', 'returns empty string as is');
assert.equal(normalizeUri(null as unknown as string), null, 'returns null as is');

const encodedUri = 'file:///c:/path/%D0%9F%D1%80%D0%BE%D0%B5%D0%BA%D1%82/main.bsl';
assert.equal(normalizeUri(encodedUri), 'file:///c:/path/Проект/main.bsl', 'decodes URI encoded characters');

const malformedUri = 'file:///c:/path/%E0%A4%A';
assert.equal(normalizeUri(malformedUri), malformedUri, 'handles decoding errors gracefully');

const backslashUri = 'file:///c:\\path\\to\\file';
assert.equal(normalizeUri(backslashUri), 'file:///c:/path/to/file', 'normalizes backslashes');

assert.equal(normalizeUri('file:///C:/Users/User/main.bsl'), 'file:///c:/Users/User/main.bsl', 'lowercases C:');
assert.equal(normalizeUri('file:///D:/Projects/main.bsl'), 'file:///d:/Projects/main.bsl', 'lowercases D:');
assert.equal(normalizeUri('file:///C:/Users/NAME/File.bsl'), 'file:///c:/Users/NAME/File.bsl', 'keeps other paths intact');

const combinedUri = 'file:///C:\\%D0%A2%D0%B5%D1%81%D1%82\\path.bsl';
assert.equal(normalizeUri(combinedUri), 'file:///c:/Тест/path.bsl', 'combines all logic');

console.log('✅ PASS  normalizeUri');
