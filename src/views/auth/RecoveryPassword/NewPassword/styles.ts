import styled from 'styled-components'

export const NewContent = styled.div`
  width: 100vw;
  height: 100vh;
  display: flex;
  justify-content: center;
  align-items: center;

  .main {
    display: flex;
    justify-content: center;
    align-items: center;
    flex-direction: column;
    gap: 10px;
    width: 25%;

    p {
      color: #ffffff;
      font-family: 'Poppins', sans-serif;
    }

    p {
      width: 300px;
      font-weight: 600;
      text-align: center;
    }

    .email {
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      position: relative;
      height: 50px;
      width: 100%;

      label {
        top: 0;
        left: 5px;
        font-size: 0.75rem;
        color: #ffffff;
        font-weight: 700;
        position: absolute;
        margin: 0 0 0 7px;
        padding: 0 3px;
        background: #252423;
        border-radius: 50px;
        width: fit-content;
      }

      input[type='text'] {
        padding: 11px 10px;
        font-size: 0.75rem;
        border: 2px #ffffff solid;
        border-radius: 5px;
        background: #363432;
        width: 100%;
        color: #ffffff;

        &:focus {
          outline: none;
        }
      }
    }
  }
`
