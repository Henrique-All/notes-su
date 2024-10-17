import { createGlobalStyle } from 'styled-components'
import 'react-toastify/dist/ReactToastify.css'

export default createGlobalStyle`
  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
    user-select: none;
  }
  @media (max-width: 1800px) {
    html {
      font-size: 93.75%;
    }
  }
  @media (max-width: 720px) {
    html {
      font-size: 87.5%;
    }
  }

  a {
    text-decoration: none;
  }
  li {
    list-style: none;
  }

  body .Toastify .Toastify__toast-container .Toastify__toast--success {
    background: rgba(255, 255, 255, 1);
    color: rgba(3, 134, 0, 1);
    font-weight: bold;
    font-size: 15pt;
    position: 'top-center',
  }
  body .Toastify .Toastify__toast-container .Toastify__toast--error {


    background: rgba(255, 255, 255, 1);
    color: #ed5247;
    font-weight: bold;
    font-size: 15pt;
    position: 'top-center',
  }


  html {
    background-color: #363432;
     // Scroll
  ::-webkit-scrollbar {
    width: 10px;
  }
  ::-webkit-scrollbar-track {
    box-shadow: inset 0 0 5px grey;
    border-radius: 10px;
  }
  // color scroll
  ::-webkit-scrollbar-thumb {
    background: #ccc;
    border-radius: 10px;
  }
  ::-webkit-scrollbar-thumb:hover {
    background: #ccc;
  }
  }
`
