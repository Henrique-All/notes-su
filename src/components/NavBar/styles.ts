import styled from 'styled-components'

interface NavProps {
  isActive: boolean
}

export const NavButton = styled.button<NavProps>`
  display: flex;
  background: #eff3f8;
  justify-content: center;
  align-items: center;
  background-color: ${(props) => (props.isActive ? '#176775' : 'transparent')};
  border: none;
  cursor: pointer;
  padding: 5px;
  border-radius: 4px;
`

export const NavLeft = styled.div`
  display: flex;
  height: 100vh;
  width: 50px;
  background-color: #252423;
  flex-direction: column;
  justify-content: center;
  align-items: center;
  position: relative;

  .logo {
    position: absolute;
    top: 10px;

    img {
      width: 35px;
    }
  }

  .center {
    display: flex;
    flex-direction: column;
    gap: 40px;

    img {
      width: 25px;
    }
  }

  .user {
    position: absolute;
    bottom: 10px;

    img {
      width: 25px;
    }
  }
`

export const Container = styled.div`
  display: flex;
`
