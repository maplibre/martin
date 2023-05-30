import styled from 'styled-components';

export default styled.div`
  position: relative;
  max-width: 400px;
  margin-bottom: 80px;

  font-size: 50px;
  font-weight: bold;
  color: transparent;
  line-height: 1.3;
  -webkit-text-stroke-width: 1px;
  -webkit-text-stroke-color: white;
  text-transform: uppercase;
  
  @media (max-width: 500px) {
    font-size: 35px;
  }
  
  img {
    max-width: 100%;
  }
`;
