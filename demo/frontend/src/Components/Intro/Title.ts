import styled from 'styled-components';

export default styled.h1`
  margin: 0 0 20px;
  
  font-size: 80px;
  letter-spacing: 15px;
  color: transparent;
  -webkit-text-stroke-width: 2px;
  -webkit-text-stroke-color: white;
  text-transform: uppercase;
  
  @media (max-width: 500px) {
    font-size: 50px;
    letter-spacing: initial;
    -webkit-text-stroke-width: 1px;
  }
`;
