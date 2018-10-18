import styled from 'styled-components';

export default styled.div`
  font-size: 16px;
  font-weight: bold;
  color: ${({isEnabled}) => (isEnabled ? '#dadfee' : '#6c7495')};
`;
