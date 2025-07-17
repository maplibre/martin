import Container from './Container';
import octocat from './octocat.svg';

const GitHubButton = () => (
  <Container href="https://github.com/maplibre/martin" target="_blank">
    <span>View on GitHub</span>
    <img alt="octocat" src={octocat} />
  </Container>
);

export default GitHubButton;
