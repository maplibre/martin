import martinCover from './assets/logo.png';
import './App.css';
import styled from 'styled-components';

const CoverImage = styled.img`
  width: 100%;
  height: 100%;
`;

function App() {
  return (
    <div>
      <CoverImage src={martinCover} />
    </div>
  );
}

export default App;
