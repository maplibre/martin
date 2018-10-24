import styled from 'styled-components';

export default styled.div`
  position: relative;
  max-width: 400px;
  margin-bottom: 80px;

  font-size: 45px;
  font-weight: bold;
  color: transparent;
  line-height: 55px;
  -webkit-text-stroke-width: 1px;
  -webkit-text-stroke-color: white;
  text-transform: uppercase;
  
  @media (max-width: 500px) {
    font-size: 35px;
  }
`;
