import GitHubButton from '../GitHubButton';
import DocsButton from '../GitHubButton/DocsButton';
import Install from '../Install';
import Container from './Container';
import Title from './Title';

const Development = () => (
  <Container>
    <Title>Start building with Martin!</Title>
    <Install />
    <GitHubButton /> <DocsButton />
  </Container>
);

export default Development;
