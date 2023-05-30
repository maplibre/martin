import React from 'react';

import Container from './Container';
import Title from './Title';
import GitHubButton from '../GitHubButton';
import DocsButton from '../GitHubButton/DocsButton';

const Development = () => (
  <Container>
    <Title>Start building with Martin!</Title>
    <GitHubButton />{' '}
    <DocsButton />
  </Container>
);

export default Development;
