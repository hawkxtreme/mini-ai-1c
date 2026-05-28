import test from 'node:test';
import assert from 'node:assert/strict';
import { shouldResetApiKeyDraft } from '../profileSecretDraft';

test('resets API key draft when a profile is opened for the first time', () => {
    assert.equal(shouldResetApiKeyDraft(null, 'profile-openai'), true);
});

test('resets API key draft when switching to another profile', () => {
    assert.equal(shouldResetApiKeyDraft('profile-openai', 'profile-deepseek'), true);
});

test('keeps API key draft when the same profile is refreshed from storage', () => {
    assert.equal(shouldResetApiKeyDraft('profile-openai', 'profile-openai'), false);
});

test('keeps API key draft when there is no selected profile', () => {
    assert.equal(shouldResetApiKeyDraft('profile-openai', null), false);
});
