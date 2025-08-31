import { Parallax } from 'react-scroll-parallax';
import GitHubButton from '../GitHubButton';
import DocsButton from '../GitHubButton/DocsButton';
import Container from './Container';
import Description from './Description';
import Title from './Title';

const Intro = () => (
  <Container>
    <Parallax translateY={[0, -50]}>
      <Title>Demo</Title>
      <Description>Vector Tiles from Large Databases on the Fly</Description>
      <GitHubButton /> <DocsButton />
    </Parallax>
  </Container>
);

export default Intro;
